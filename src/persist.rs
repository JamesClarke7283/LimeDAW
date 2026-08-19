//! Saving and loading: projects, rendered audio and MIDI.

use crate::model::{Id, Note, Pattern, Project};

/// Write interleaved stereo samples to a 32-bit float WAV.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_wav(path: &std::path::Path, samples: &[f32], sample_rate: u32) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for sample in samples {
        writer.write_sample(*sample)?;
    }
    writer.finalize()?;
    Ok(())
}

/// Serialise a project to the on-disk JSON format.
pub fn project_to_json(project: &Project) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(project)?)
}

/// Parse a project, tolerating files written by older versions.
pub fn project_from_json(raw: &str) -> anyhow::Result<Project> {
    let mut project: Project = serde_json::from_str(raw)?;
    project.sanitise();
    Ok(project)
}

// ---------------------------------------------------------------------------
// Standard MIDI files
// ---------------------------------------------------------------------------

/// Ticks per quarter note used when writing MIDI.
const MIDI_PPQ: u16 = 480;

fn write_var_len(out: &mut Vec<u8>, mut value: u32) {
    let mut buffer = vec![(value & 0x7F) as u8];
    value >>= 7;
    while value > 0 {
        buffer.push(((value & 0x7F) | 0x80) as u8);
        value >>= 7;
    }
    out.extend(buffer.iter().rev().copied());
}

/// Export one pattern as a type-0 Standard MIDI File.
///
/// Each channel becomes a MIDI channel, in rack order, skipping channel 10
/// unless the source channel is percussion.
pub fn pattern_to_midi(project: &Project, pattern: &Pattern) -> Vec<u8> {
    #[derive(Clone, Copy)]
    struct Event {
        tick: u32,
        status: u8,
        data1: u8,
        data2: u8,
    }

    let mut events: Vec<Event> = Vec::new();
    for note in &pattern.notes {
        let Some(index) = project.channel_index(note.channel) else {
            continue;
        };
        let midi_channel = (index % 16) as u8;
        let start = (note.start * MIDI_PPQ as f64).round().max(0.0) as u32;
        let end = (note.end() * MIDI_PPQ as f64).round().max(0.0) as u32;
        let velocity = (note.velocity * 127.0).clamp(1.0, 127.0) as u8;
        events.push(Event { tick: start, status: 0x90 | midi_channel, data1: note.key, data2: velocity });
        events.push(Event { tick: end.max(start + 1), status: 0x80 | midi_channel, data1: note.key, data2: 0 });
    }
    // Note-offs sort before note-ons at the same tick so repeats retrigger.
    events.sort_by_key(|e| (e.tick, if e.status & 0xF0 == 0x80 { 0 } else { 1 }));

    let mut track = Vec::new();

    // Tempo meta event.
    let micros_per_quarter = (60_000_000.0 / project.tempo.max(1.0)) as u32;
    write_var_len(&mut track, 0);
    track.extend_from_slice(&[0xFF, 0x51, 0x03]);
    track.extend_from_slice(&micros_per_quarter.to_be_bytes()[1..4]);

    // Time signature meta event.
    write_var_len(&mut track, 0);
    let denominator_power = (project.time_signature.denominator as f32).log2().round() as u8;
    track.extend_from_slice(&[
        0xFF,
        0x58,
        0x04,
        project.time_signature.numerator as u8,
        denominator_power,
        24,
        8,
    ]);

    let mut last_tick = 0u32;
    for event in events {
        write_var_len(&mut track, event.tick.saturating_sub(last_tick));
        track.extend_from_slice(&[event.status, event.data1, event.data2]);
        last_tick = event.tick;
    }
    write_var_len(&mut track, 0);
    track.extend_from_slice(&[0xFF, 0x2F, 0x00]);

    let mut out = Vec::new();
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // format 0
    out.extend_from_slice(&1u16.to_be_bytes()); // one track
    out.extend_from_slice(&MIDI_PPQ.to_be_bytes());
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&(track.len() as u32).to_be_bytes());
    out.extend_from_slice(&track);
    out
}

/// Read a Standard MIDI File into notes, one per MIDI note-on/off pair.
///
/// Returns the notes in beats, along with the tempo if the file declares one.
pub fn midi_to_notes(bytes: &[u8], channel: Id, next_id: &mut Id) -> anyhow::Result<(Vec<Note>, Option<f64>)> {
    if bytes.len() < 14 || &bytes[0..4] != b"MThd" {
        anyhow::bail!("not a MIDI file");
    }
    let division = u16::from_be_bytes([bytes[12], bytes[13]]);
    if division & 0x8000 != 0 {
        anyhow::bail!("SMPTE timing is not supported");
    }
    let ticks_per_beat = division.max(1) as f64;

    let mut cursor = 14usize;
    let mut notes: Vec<Note> = Vec::new();
    let mut tempo = None;

    while cursor + 8 <= bytes.len() {
        let chunk_type = &bytes[cursor..cursor + 4];
        let length = u32::from_be_bytes([
            bytes[cursor + 4],
            bytes[cursor + 5],
            bytes[cursor + 6],
            bytes[cursor + 7],
        ]) as usize;
        cursor += 8;
        let end = (cursor + length).min(bytes.len());
        if chunk_type != b"MTrk" {
            cursor = end;
            continue;
        }

        let mut pos = cursor;
        let mut tick = 0u64;
        let mut running_status = 0u8;
        // Notes waiting for their note-off, keyed by (channel, key).
        let mut pending: Vec<(u8, u8, u64, u8)> = Vec::new();

        while pos < end {
            // Delta time.
            let mut delta = 0u32;
            loop {
                if pos >= end {
                    break;
                }
                let byte = bytes[pos];
                pos += 1;
                delta = (delta << 7) | (byte & 0x7F) as u32;
                if byte & 0x80 == 0 {
                    break;
                }
            }
            tick += delta as u64;
            if pos >= end {
                break;
            }

            let mut status = bytes[pos];
            if status & 0x80 != 0 {
                pos += 1;
                running_status = status;
            } else {
                status = running_status;
            }

            match status & 0xF0 {
                0x80 | 0x90 => {
                    if pos + 1 >= end {
                        break;
                    }
                    let key = bytes[pos];
                    let velocity = bytes[pos + 1];
                    pos += 2;
                    let midi_channel = status & 0x0F;
                    let is_on = status & 0xF0 == 0x90 && velocity > 0;
                    if is_on {
                        pending.push((midi_channel, key, tick, velocity));
                    } else if let Some(index) = pending
                        .iter()
                        .position(|(c, k, _, _)| *c == midi_channel && *k == key)
                    {
                        let (_, key, start, velocity) = pending.remove(index);
                        let start_beats = start as f64 / ticks_per_beat;
                        let length = (tick as f64 / ticks_per_beat - start_beats).max(1.0 / 64.0);
                        let id = *next_id;
                        *next_id += 1;
                        notes.push(Note {
                            id,
                            channel,
                            key,
                            start: start_beats,
                            length,
                            velocity: velocity as f32 / 127.0,
                            pan: 0.0,
                            fine_tune: 0.0,
                            release: 0.0,
                        });
                    }
                }
                0xA0 | 0xB0 | 0xE0 => pos += 2,
                0xC0 | 0xD0 => pos += 1,
                0xF0 => {
                    if status == 0xFF {
                        if pos >= end {
                            break;
                        }
                        let meta = bytes[pos];
                        pos += 1;
                        let mut len = 0usize;
                        loop {
                            if pos >= end {
                                break;
                            }
                            let byte = bytes[pos];
                            pos += 1;
                            len = (len << 7) | (byte & 0x7F) as usize;
                            if byte & 0x80 == 0 {
                                break;
                            }
                        }
                        if meta == 0x51 && len == 3 && pos + 3 <= end {
                            let micros = ((bytes[pos] as u32) << 16)
                                | ((bytes[pos + 1] as u32) << 8)
                                | bytes[pos + 2] as u32;
                            if micros > 0 {
                                tempo = Some(60_000_000.0 / micros as f64);
                            }
                        }
                        pos += len;
                    } else {
                        // SysEx: skip its declared length.
                        let mut len = 0usize;
                        loop {
                            if pos >= end {
                                break;
                            }
                            let byte = bytes[pos];
                            pos += 1;
                            len = (len << 7) | (byte & 0x7F) as usize;
                            if byte & 0x80 == 0 {
                                break;
                            }
                        }
                        pos += len;
                    }
                }
                _ => break,
            }
        }
        cursor = end;
    }

    notes.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    Ok((notes, tempo))
}
