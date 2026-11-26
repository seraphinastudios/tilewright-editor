// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A person's favourites in the desktop (docs/OFFLINE-PROTOTYPE.md#favourites):
//! read from the private `palettes/` directory the provisioning helper
//! passes (`--palettes-fd`), shown first in Terrain and Place mode, and
//! written back as a new file in that directory after every change. Never
//! a path outside the session: the helper copies the newest file back.
//!
//! A favourite row is the catalog row of the same value, with the same
//! control, so choosing it prepares exactly what choosing that value in the
//! catalog's list prepares. Nothing is recorded about favourites, and they
//! never reach frame evidence: the scripted demonstration refuses them.
use super::{Application, IdText, Result, UiRows};
use crate::workspace_ui::Control;
use tilewright_editor_ui::favourites::{Favourite, Favourites, MAXIMUM_BYTES};
use tilewright_editor_ui::tools::ToolMode;
use tilewright_source::{Access, AccessError, Mounts, SourceLock, SourceRoot};

/// The file the helper copies the person's favourites to.
pub(super) const FILE: &str = "favourites.json";
/// Most files one session writes back: `favourites-0001.json` on.
pub(super) const MAXIMUM_SAVES: u32 = 9999;
pub(super) const HEADER: &str = "Favourites";
pub(super) const EMPTY: &str = "None yet: choose, then Star";
/// Before a favourite this catalog does not grant, and the line above why
/// a malformed file was ignored.
pub(super) const NOT_GRANTED: &str = "Not granted: ";
pub(super) const IGNORED: &str = "Favourites file ignored:";
/// About as many characters of the ignored file's reason as a sidebar row
/// holds; a longer word is still elided when laid out.
const REASON_ROW: usize = 30;

/// `reason` in rows of at most [`REASON_ROW`] characters, broken at spaces.
fn reason_rows(reason: &str) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    for word in reason.split(' ') {
        match rows.last_mut() {
            Some(row) if row.chars().count() + 1 + word.chars().count() <= REASON_ROW => {
                row.push(' ');
                row.push_str(word);
            }
            _ => rows.push(word.to_owned()),
        }
    }
    rows
}
pub(super) const STAR: &str = "Star the chosen value";
pub(super) const UNSTAR: &str = "Unstar the chosen value";
pub(super) const RAISE: &str = "Move it up the favourites";

/// The name of the session's `index`th written-back file.
pub(super) fn saved_name(index: u32) -> String {
    format!("favourites-{index:04}.json")
}

/// The category of an access error, never a path.
fn detail(error: &AccessError) -> String {
    match error {
        AccessError::Path(_) => "invalid relative name".into(),
        AccessError::Link(_) => "symbolic link refused".into(),
        AccessError::Unexpected(_) => "not a regular file or directory".into(),
        AccessError::TooLarge(_) => "bounded access limit exceeded".into(),
        AccessError::Locked => "another holder has the palettes directory's lock".into(),
        AccessError::Io(_, error) => format!(
            "filesystem I/O {:?} (OS {:?})",
            error.kind(),
            error.raw_os_error()
        ),
    }
}

/// The session's favourites: the directory, held locked, and the lists, or
/// why the file was ignored.
pub(super) struct Personal {
    root: SourceRoot,
    _lock: SourceLock,
    access: Access,
    lists: std::result::Result<Favourites, String>,
    /// How many files this session wrote back.
    saves: u32,
}

impl Personal {
    /// Opens the provisioned `palettes/` directory and reads its
    /// favourites. A directory that is not this user's alone (mode 0700, no
    /// access control list, a checked local file system), a missing, linked
    /// or not private (0600) file is refused; a malformed one is ignored as
    /// a whole, with its reason.
    pub(super) fn open(root: SourceRoot) -> Result<Self> {
        let refuse = |reason: String| format!("favourites refused: {reason}");
        let directory = root
            .directory("", false)
            .map_err(|error| refuse(detail(&error)))?
            .ok_or_else(|| refuse("the palettes directory is missing".into()))?;
        let access = directory
            .own_access()
            .map_err(|error| refuse(detail(&error)))?;
        if access.mode != 0o700 || access.user != access.new_file().user {
            return Err(refuse(
                "the palettes directory must be the current user's with mode exactly 0700, without access control lists".into(),
            ));
        }
        if directory
            .unchecked_file_system(&Mounts::read())
            .map_err(|error| refuse(detail(&error)))?
            .is_some()
        {
            return Err(refuse(
                "the palettes directory is not on a checked local file system".into(),
            ));
        }
        let lock = root.lock().map_err(|error| refuse(detail(&error)))?;
        match directory
            .access(FILE)
            .map_err(|error| refuse(detail(&error)))?
        {
            None => return Err(refuse(format!("the palettes directory has no {FILE}"))),
            Some(file) if file.mode != 0o600 || file.user != access.user => {
                return Err(refuse(format!(
                    "{FILE} must be the current user's with mode exactly 0600"
                )));
            }
            Some(_) => {}
        }
        let lists = match root.read(FILE, MAXIMUM_BYTES as u64) {
            Ok(Some(bytes)) => Favourites::decode(&bytes).map_err(|error| error.to_string()),
            Ok(None) => return Err(refuse(format!("the palettes directory has no {FILE}"))),
            Err(AccessError::TooLarge(_)) => Err(format!(
                "the file is larger than {} KiB",
                MAXIMUM_BYTES / 1024
            )),
            Err(error) => return Err(refuse(detail(&error))),
        };
        Ok(Self {
            root,
            _lock: lock,
            access,
            lists,
            saves: 0,
        })
    }

    /// The lists, or why the file was ignored.
    pub(super) fn lists(&self) -> std::result::Result<&Favourites, &str> {
        self.lists.as_ref().map_err(String::as_str)
    }

    /// How many files this session wrote back.
    pub(super) fn saves(&self) -> u32 {
        self.saves
    }

    /// The start-up line: what was read, by count only, or why it was
    /// ignored.
    pub(super) fn summary(&self) -> String {
        match &self.lists {
            Ok(lists) => format!(
                "favourites: {} terrain, {} scatter, {} objects",
                lists.terrain.len(),
                lists.scatter.len(),
                lists.objects.len()
            ),
            Err(reason) => format!("favourites: file ignored: {reason}"),
        }
    }

    /// Writes `next` as the session's next new file (create-only: a file
    /// already at that name is never replaced) and syncs the directory;
    /// only then are they the session's lists.
    fn retain(&mut self, next: Favourites) -> Result<()> {
        if self.lists.is_err() {
            return Err("the favourites file was ignored, so nothing is saved".into());
        }
        let index = self.saves + 1;
        if index > MAXIMUM_SAVES {
            return Err(format!(
                "this session already wrote {MAXIMUM_SAVES} favourites files"
            ));
        }
        let name = saved_name(index);
        let directory = self
            .root
            .directory("", false)
            .map_err(|error| detail(&error))?
            .ok_or("the palettes directory is missing")?;
        directory
            .create(&name, Some(&self.access.new_file()), &next.encode())
            .map_err(|error| {
                if error.already_exists() {
                    format!("{name} is already in the session; nothing was replaced")
                } else {
                    detail(&error)
                }
            })?;
        if !directory.sync().map_err(|error| detail(&error))? {
            return Err("the palettes directory could not be synced".into());
        }
        self.saves = index;
        self.lists = Ok(next);
        Ok(())
    }
}

impl Application {
    /// The value the mode's star row stars: Terrain mode's chosen terrain
    /// or scatter palette, Place mode's chosen prototype.
    fn chosen_favourite(&self) -> Option<Favourite> {
        match self.tools.mode() {
            ToolMode::Terrain => self
                .tools
                .terrain_choice()
                .cloned()
                .map(Favourite::Terrain)
                .or_else(|| {
                    self.tools
                        .scatter_choice()
                        .map(|name| Favourite::Scatter(name.to_owned()))
                }),
            ToolMode::Place => self
                .tools
                .placement_choice()
                .map(|name| Favourite::Object(name.to_owned())),
            _ => None,
        }
    }

    /// Stars or unstars the chosen value, or moves it up, and writes the
    /// lists back. A refusal leaves the lists as they were.
    pub(super) fn favourite_control(&mut self, raise: bool) -> Result<()> {
        let refuse = |reason: String| format!("Favourites not saved: {reason}");
        let Some(personal) = self.favourites.as_ref() else {
            return Err(refuse("this session has no favourites file".into()));
        };
        let Some(chosen) = self.chosen_favourite() else {
            return Err(refuse("choose a value first".into()));
        };
        let mut next = personal
            .lists()
            .map_err(|_| refuse("the favourites file was ignored, so nothing is saved".into()))?
            .clone();
        if raise {
            if !next.raise(&chosen) {
                return Err(refuse("it is already first, or not a favourite".into()));
            }
        } else {
            next.toggle(&chosen)
                .map_err(|error| refuse(error.to_string()))?;
        }
        self.favourites
            .as_mut()
            .map_or(Ok(()), |personal| personal.retain(next))
            .map_err(refuse)
    }

    /// The mode's favourites section, before its catalog lists, only with a
    /// favourites file: each favourite as the catalog row of the same value
    /// (or disabled, with the reason, when this catalog does not grant it),
    /// then the star row and, for a starred value below the first, the row
    /// that moves it up.
    pub(super) fn favourite_rows(
        &self,
        rows: &mut UiRows,
        fits: &mut Vec<(usize, IdText)>,
        ready: bool,
    ) {
        let Some(personal) = &self.favourites else {
            return;
        };
        rows.push((HEADER.into(), None, true, false));
        let lists = match personal.lists() {
            Ok(lists) => lists,
            Err(reason) => {
                rows.push((IGNORED.into(), None, false, false));
                for row in reason_rows(reason) {
                    rows.push((row, None, false, false));
                }
                return;
            }
        };
        let place = self.tools.mode() == ToolMode::Place;
        let entries = lists.entries(place);
        if entries.is_empty() {
            rows.push((EMPTY.into(), None, false, false));
        }
        let catalog = self.session.catalog();
        for favourite in &entries {
            if !favourite.granted(catalog) {
                let name = match favourite {
                    Favourite::Terrain(choice) => format!("{} / {}", choice.grid, choice.prototype),
                    Favourite::Scatter(name) => format!("scatter {name}"),
                    Favourite::Object(name) => name.clone(),
                };
                // Fitted later so the reason and the value's tail both stay.
                fits.push((
                    rows.len(),
                    IdText::Id {
                        id: name.clone(),
                        variants: vec![(NOT_GRANTED.into(), String::new())],
                    },
                ));
                rows.push((format!("{NOT_GRANTED}{name}"), None, false, false));
                continue;
            }
            match favourite {
                Favourite::Terrain(choice) => self.terrain_row(rows, fits, choice.clone(), ready),
                Favourite::Scatter(name) => self.scatter_row(rows, name, ready),
                Favourite::Object(name) => self.place_row(rows, name, ready),
            }
        }
        let chosen = self.chosen_favourite();
        let starred = chosen.as_ref().and_then(|chosen| lists.position(chosen));
        rows.push((
            if starred.is_some() { UNSTAR } else { STAR }.into(),
            Some(Control::FavouriteStar),
            chosen.is_some(),
            starred.is_some(),
        ));
        if starred.is_some_and(|index| index > 0) {
            rows.push((RAISE.into(), Some(Control::FavouriteRaise), true, false));
        }
        // The helper copies the newest saved file back when the editor ends.
        if personal.saves() > 0 {
            rows.push((
                format!("Saved: {}", saved_name(personal.saves())),
                None,
                true,
                false,
            ));
        }
    }
}
