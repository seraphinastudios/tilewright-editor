// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Multi-object stamps ([`crate::Action::PlaceObjects`], ADR 0009): several
//! new objects, each a granted prototype at an offset from one anchor tile,
//! placed in one reviewed World transaction. The members are copied into
//! the action, never named: a catalog stamp and a copy captured from the
//! map are only two sources of the same list. Any rotation or mirroring is
//! applied to the offsets before the action is made, so nothing here turns
//! or flips anything.
use tilewright_source::World;

use crate::session::{RefusalCode, SessionError, refusal::refuse};

/// Most objects one stamp places.
pub const MAXIMUM_STAMP_OBJECTS: usize = 64;
/// The largest offset of a member from the anchor tile, on either axis
/// and either way. A view holds at most 4,096 tiles, so no two of its
/// tiles are further apart than 4,095 on one axis: every stamp that fits
/// in a view fits this bound.
pub const MAXIMUM_STAMP_OFFSET: i64 = 4_095;

/// One member of a stamp: a prototype and its offset `[dx, dy]` from the
/// anchor tile, after any rotation or mirroring.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StampObject {
    pub proto: String,
    pub offset: [i64; 2],
}

/// The engine's rule for a stamp's members: 1 to
/// [`MAXIMUM_STAMP_OBJECTS`] (none is `input.invalid`, more is
/// `input.budget`), each offset at most [`MAXIMUM_STAMP_OFFSET`] either way
/// on both axes (`input.budget`). Members may repeat: the World format
/// stacks objects on a tile, so two on one tile are valid.
///
/// # Errors
/// The refusal, as above.
pub fn check_objects(objects: &[StampObject]) -> Result<(), SessionError> {
    if objects.is_empty() {
        return Err(refuse(
            RefusalCode::InputInvalid,
            "a stamp places at least one object",
        ));
    }
    if objects.len() > MAXIMUM_STAMP_OBJECTS {
        return Err(refuse(
            RefusalCode::InputBudget,
            &format!("a stamp places at most {MAXIMUM_STAMP_OBJECTS} objects"),
        ));
    }
    if let Some(index) = objects
        .iter()
        .position(|object| !offset_in_bounds(object.offset))
    {
        return Err(refuse(
            RefusalCode::InputBudget,
            &format!(
                "stamp object {} is offset more than {MAXIMUM_STAMP_OFFSET} tiles from the anchor",
                index + 1
            ),
        ));
    }
    Ok(())
}

/// Whether `offset` is within [`MAXIMUM_STAMP_OFFSET`] on both axes.
pub fn offset_in_bounds(offset: [i64; 2]) -> bool {
    offset
        .iter()
        .all(|axis| axis.unsigned_abs() <= MAXIMUM_STAMP_OFFSET.unsigned_abs())
}

/// The plane tile of a member at `offset` from the anchor `x, y`, or
/// `None` when it would overflow (and so lies outside every view).
pub fn member_tile([x, y]: [i64; 2], offset: [i64; 2]) -> Option<[i64; 2]> {
    Some([x.checked_add(offset[0])?, y.checked_add(offset[1])?])
}

/// How many members land on an occupied tile: one where the `committed`
/// World already holds a top-level object on that plane and spatial
/// layer, or where an earlier member of the same stamp lands. Members
/// whose tile overflows are not counted.
pub fn occupied(
    committed: &World,
    plane: &str,
    layer: i32,
    anchor: [i64; 2],
    objects: &[StampObject],
) -> usize {
    let mut landed: Vec<[i64; 2]> = Vec::with_capacity(objects.len());
    let mut count = 0;
    for object in objects {
        let Some(tile) = member_tile(anchor, object.offset) else {
            continue;
        };
        if landed.contains(&tile) || holds_object(committed, plane, layer, tile) {
            count += 1;
        }
        landed.push(tile);
    }
    count
}

/// The review warning for a stamp with members on occupied tiles
/// ([`occupied`]), or `None` when none is. It never refuses anything: the
/// World format stacks objects, and each new one goes on top.
pub fn occupied_warning(
    committed: &World,
    plane: &str,
    layer: i32,
    anchor: [i64; 2],
    objects: &[StampObject],
) -> Option<String> {
    let count = occupied(committed, plane, layer, anchor, objects);
    (count > 0).then(|| {
        let total = objects.len();
        format!(
            "Warning: {count} of the {total} new object{} land{} on {} that already hold{} an object or another new one; each is added on top.",
            if total == 1 { "" } else { "s" },
            if count == 1 { "s" } else { "" },
            if count == 1 { "a tile" } else { "tiles" },
            if count == 1 { "s" } else { "" },
        )
    })
}

/// Whether plane tile `at` of `plane` and spatial layer `layer` holds a
/// top-level object in `world`.
fn holds_object(world: &World, plane: &str, layer: i32, at: [i64; 2]) -> bool {
    let Some(plane) = world.planes().get(plane) else {
        return false;
    };
    let size = i64::from(plane.chunk_size);
    if size <= 0 {
        return false;
    }
    let (Ok(lx), Ok(ly)) = (
        u16::try_from(at[0].rem_euclid(size)),
        u16::try_from(at[1].rem_euclid(size)),
    ) else {
        return false;
    };
    plane
        .chunks
        .get(&(layer, at[0].div_euclid(size), at[1].div_euclid(size)))
        .is_some_and(|chunk| {
            chunk
                .objects
                .iter()
                .any(|object| object.at == Some([lx, ly]))
        })
}

#[cfg(test)]
mod tests {
    use super::{MAXIMUM_STAMP_OFFSET, StampObject, check_objects, member_tile};
    use crate::RefusalCode;

    fn member(dx: i64, dy: i64) -> StampObject {
        StampObject {
            proto: "marker".into(),
            offset: [dx, dy],
        }
    }

    #[test]
    fn members_are_one_to_sixty_four_within_the_offset_bound() {
        assert_eq!(
            check_objects(&[]).unwrap_err().code(),
            RefusalCode::InputInvalid
        );
        assert!(check_objects(&vec![member(0, 0); 64]).is_ok());
        assert_eq!(
            check_objects(&vec![member(0, 0); 65]).unwrap_err().code(),
            RefusalCode::InputBudget
        );
        let edge = MAXIMUM_STAMP_OFFSET;
        assert!(check_objects(&[member(edge, -edge), member(-edge, edge)]).is_ok());
        for offset in [[edge + 1, 0], [0, -edge - 1], [i64::MIN, 0], [0, i64::MAX]] {
            let error = check_objects(&[member(0, 0), member(offset[0], offset[1])]).unwrap_err();
            assert_eq!(error.code(), RefusalCode::InputBudget, "{offset:?}");
            assert!(error.to_string().contains("object 2"), "{error}");
        }
    }

    #[test]
    fn a_member_tile_that_overflows_is_none() {
        assert_eq!(member_tile([3, 4], [-1, 2]), Some([2, 6]));
        assert_eq!(member_tile([i64::MAX, 0], [1, 0]), None);
        assert_eq!(member_tile([0, i64::MIN], [0, -1]), None);
    }
}
