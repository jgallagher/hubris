pub(super) struct Song<'a> {
    pub(super) notes: &'a [Note],
    pub(super) sixteenth_ms: u64,
    pub(super) between_notes_ms: u64,
}

pub(super) const TWINKLE_TWINKLE: Song<'static> = Song {
    notes: &[
        Note::new(NoteName::C, 3, NoteDuration::Quarter),
        Note::new(NoteName::C, 3, NoteDuration::Quarter),
        Note::new(NoteName::G, 3, NoteDuration::Quarter),
        Note::new(NoteName::G, 3, NoteDuration::Quarter),
        Note::new(NoteName::A, 4, NoteDuration::Quarter),
        Note::new(NoteName::A, 4, NoteDuration::Quarter),
        Note::new(NoteName::G, 3, NoteDuration::Half),
        Note::new(NoteName::F, 3, NoteDuration::Quarter),
        Note::new(NoteName::F, 3, NoteDuration::Quarter),
        Note::new(NoteName::E, 3, NoteDuration::Quarter),
        Note::new(NoteName::E, 3, NoteDuration::Quarter),
        Note::new(NoteName::D, 3, NoteDuration::Quarter),
        Note::new(NoteName::D, 3, NoteDuration::Quarter),
        Note::new(NoteName::C, 3, NoteDuration::Half),
    ],
    sixteenth_ms: tempo_to_sixteenth_ms(180),
    between_notes_ms: tempo_to_sixteenth_ms(180) / 2,
};

pub(super) const FUR_ELISE: Song<'static> = Song {
    notes: &[
        Note::new(NoteName::E, 4, NoteDuration::Sixteenth),
        Note::new(NoteName::Eb, 4, NoteDuration::Sixteenth),
        // bar 1
        Note::new(NoteName::E, 4, NoteDuration::Sixteenth),
        Note::new(NoteName::Eb, 4, NoteDuration::Sixteenth),
        Note::new(NoteName::E, 4, NoteDuration::Sixteenth),
        Note::new(NoteName::B, 4, NoteDuration::Sixteenth),
        Note::new(NoteName::D, 4, NoteDuration::Sixteenth),
        Note::new(NoteName::C, 4, NoteDuration::Sixteenth),
        // bar 2
        Note::new(NoteName::A, 4, NoteDuration::Eighth),
        Note::rest(NoteDuration::Sixteenth),
        Note::new(NoteName::C, 3, NoteDuration::Sixteenth),
        Note::new(NoteName::E, 3, NoteDuration::Sixteenth),
        Note::new(NoteName::A, 4, NoteDuration::Sixteenth),
        // bar 3
        Note::new(NoteName::B, 4, NoteDuration::Eighth),
        Note::rest(NoteDuration::Sixteenth),
        Note::new(NoteName::E, 3, NoteDuration::Sixteenth),
        Note::new(NoteName::Ab, 3, NoteDuration::Sixteenth),
        Note::new(NoteName::B, 4, NoteDuration::Sixteenth),
        // bar 4
        Note::new(NoteName::C, 4, NoteDuration::Eighth),
    ],
    sixteenth_ms: tempo_to_sixteenth_ms(60),
    between_notes_ms: tempo_to_sixteenth_ms(60) / 16,
};

#[derive(Clone, Copy)]
#[repr(u8)]
#[allow(dead_code)]
enum NoteName {
    A,
    Bb,
    B,
    C,
    Db,
    D,
    Eb,
    E,
    F,
    Gb,
    G,
    Ab,
}

#[derive(Clone, Copy)]
#[repr(u8)]
#[allow(dead_code)]
pub(super) enum NoteDuration {
    Sixteenth,
    Eighth,
    Quarter,
    Half,
    Whole,
}

impl NoteDuration {
    pub(super) fn sixteenths(self) -> u64 {
        match self {
            NoteDuration::Sixteenth => 1,
            NoteDuration::Eighth => 2,
            NoteDuration::Quarter => 4,
            NoteDuration::Half => 8,
            NoteDuration::Whole => 16,
        }
    }
}

pub(super) struct Note {
    pub(super) freq: Option<u16>,
    pub(super) duration: NoteDuration,
}

impl Note {
    // Note: `octave` maps [A..Ab] starting with the lowest key on the piano;
    // this isn't the standard octave numbering.
    const fn new(name: NoteName, octave: u8, duration: NoteDuration) -> Self {
        Self {
            freq: Some(note_to_freq(name, octave)),
            duration,
        }
    }

    const fn rest(duration: NoteDuration) -> Self {
        Self {
            freq: None,
            duration,
        }
    }
}

const fn tempo_to_sixteenth_ms(bpm: u64) -> u64 {
    // sixteenth/ms = bpm / (60_000 ms/sec / 4 sixteenth/beat)
    15_000 / bpm
}

// Defines `PIANO_NOTES_HZ`, an 88-long array with frequencies for each note.
// This is only used in `note_to_freq` below, and it is only called at compile
// time. This array doesn't exist in our final built task.
include!(concat!(env!("OUT_DIR"), "/piano.rs"));

const fn note_to_freq(name: NoteName, octave: u8) -> u16 {
    let index = name as usize + 12 * octave as usize;
    PIANO_NOTES_HZ[index]
}
