// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use task_jukebox_api::JukeboxError;
use drv_piezo_element_api::Piezo;
use userlib::*;

mod songs;

use self::songs::{Song, FUR_ELISE, TWINKLE_TWINKLE};

const SONGS: &'static [&'static Song<'static>] =
    &[&FUR_ELISE, &TWINKLE_TWINKLE];

task_slot!(PIEZO, piezo_element);

const TIMER_NOTIFICATION: u32 = 1 << 0;

#[export_name = "main"]
pub fn main() -> ! {
    let piezo = PIEZO.get_task_id();
    let piezo = Piezo::from(piezo);

    let mut server = ServerImpl {
        song: None,
        piezo: &piezo,
    };

    let mut buf = [0; idl::INCOMING_SIZE];
    loop {
        idol_runtime::dispatch_n(&mut buf, &mut server);
    }
}

struct ServerImpl<'a> {
    song: Option<CurrentSong>,
    piezo: &'a Piezo,
}

impl ServerImpl<'_> {
    fn update(&mut self) {
        // only have something to do if we're playing a song
        let song = match self.song.as_mut() {
            Some(song) => song,
            None => return,
        };

        // either set a new timer, or we're done
        match song.update(self.piezo) {
            SongStatus::StillPlaying(deadline) => {
                sys_set_timer(Some(deadline), TIMER_NOTIFICATION)
            }
            SongStatus::Done => self.song = None,
        }
    }
}

impl idol_runtime::NotificationHandler for ServerImpl<'_> {
    fn current_notification_mask(&self) -> u32 {
        if self.song.is_some() {
            TIMER_NOTIFICATION
        } else {
            0
        }
    }

    fn handle_notification(&mut self, _bits: u32) {
        self.update();
    }
}

impl idl::InOrderJukeboxImpl for ServerImpl<'_> {
    fn play_song(
        &mut self,
        _msg: &userlib::RecvMessage,
        song_index: usize,
    ) -> Result<(), idol_runtime::RequestError<JukeboxError>> {
        if self.song.is_some() {
            return Err(JukeboxError::BusyPlaying.into());
        }

        let song = SONGS.get(song_index).ok_or(JukeboxError::BadSongIndex)?;
        self.song = Some(CurrentSong::start(self.piezo, song));
        self.update();

        Ok(())
    }
}

struct CurrentSong {
    song: &'static Song<'static>,
    note: usize,             // index into `song.notes`
    piezo_on_deadline: u64, // we should keep piezo on the current note freq until this deadline
    piezo_off_deadline: u64, // we should keep piezo off until this deadlines; always greater than "on deadline"
}

enum SongStatus {
    StillPlaying(u64),
    Done,
}

impl CurrentSong {
    fn start(piezo: &Piezo, song: &'static Song<'static>) -> Self {
        let mut this = Self {
            song,
            note: 0,
            piezo_on_deadline: 0,
            piezo_off_deadline: 0,
        };
        this.start_current_note(piezo, sys_get_timer().now); // panics if song has 0 notes... seems fine
        this
    }

    fn start_current_note(&mut self, piezo: &Piezo, now: u64) -> u64 {
        let note = &self.song.notes[self.note];

        let duration = note.duration.sixteenths() * self.song.sixteenth_ms;
        self.piezo_off_deadline = now + duration;

        if let Some(freq) = note.freq {
            // subtract the `between_notes_ms` from the note duration so we can
            // hear a break between subsequent notes of the same pitch without
            // getting off the beat
            self.piezo_on_deadline =
                self.piezo_off_deadline - self.song.between_notes_ms;
            piezo.piezo_on(freq).unwrap();

            self.piezo_on_deadline
        } else {
            // no frequency; this is a rest
            self.piezo_on_deadline = 0;

            // piezo should already be off, but might not be if we somehow
            // didn't get a chance to run in between the `on` and `off`
            // deadliens of the previous note. go ahead and turn it off just in
            // case.
            piezo.piezo_off().unwrap();

            self.piezo_off_deadline
        }
    }

    // Updates `piezo` based on our current progress; if we're still playing,
    // returns `SongStatus::StillPlaying(deadline)` with the deadline for when
    // `update()` should be called again. It's fine to call `update()` early.
    fn update(&mut self, piezo: &Piezo) -> SongStatus {
        if self.note >= self.song.notes.len() {
            return SongStatus::Done;
        }

        // see if we're still earlier than the "on" or "off" deadlines
        let now = sys_get_timer().now;
        if now < self.piezo_on_deadline {
            return SongStatus::StillPlaying(self.piezo_on_deadline);
        }
        if now < self.piezo_off_deadline {
            piezo.piezo_off().unwrap();
            return SongStatus::StillPlaying(self.piezo_off_deadline);
        }

        // current note is done; advance to next
        self.note += 1;
        if self.note < self.song.notes.len() {
            SongStatus::StillPlaying(self.start_current_note(piezo, now))
        } else {
            piezo.piezo_off().unwrap();
            SongStatus::Done
        }
    }
}

mod idl {
    use super::JukeboxError;
    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}
