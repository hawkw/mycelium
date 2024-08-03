use super::*;
use proptest::{prop_assert_eq, proptest};

#[test]
// This test doesn't exercise anything that's potentially memory-unsafe, so
// don't spend time running it under miri.
#[cfg_attr(miri, ignore)]
fn wheel_indices() {
    let core = Core::new();
    for ticks in 0..64 {
        assert_eq!(
            core.wheel_index(ticks),
            0,
            "Core::wheel_index({ticks}) == 0"
        )
    }

    for wheel in 1..Core::WHEELS {
        for slot in wheel..Wheel::SLOTS {
            let ticks = (slot * usize::pow(Wheel::SLOTS, wheel as u32)) as u64;
            assert_eq!(
                core.wheel_index(ticks),
                wheel,
                "Core::wheel_index({ticks}) == {wheel}"
            );

            if slot > wheel {
                let ticks = ticks - 1;
                assert_eq!(
                    core.wheel_index(ticks),
                    wheel,
                    "Core::wheel_index({ticks}) == {wheel}"
                );
            }

            if slot < 64 {
                let ticks = ticks + 1;
                assert_eq!(
                    core.wheel_index(ticks),
                    wheel,
                    "Core::wheel_index({ticks}) == {wheel}"
                );
            }
        }
    }
}

#[test]
// This test doesn't exercise anything that's potentially memory-unsafe, so
// don't spend time running it under miri.
#[cfg_attr(miri, ignore)]
fn bitshift_is_correct() {
    assert_eq!(1 << Wheel::BITS, Wheel::SLOTS);
}

#[test]
// This test doesn't exercise anything that's potentially memory-unsafe, so
// don't spend time running it under miri.
#[cfg_attr(miri, ignore)]
fn slot_indices() {
    let wheel = Wheel::new(0);
    for i in 0..64 {
        let slot_index = wheel.slot_index(i);
        assert_eq!(i as usize, slot_index, "wheels[0].slot_index({i}) == {i}")
    }

    for level in 1..Core::WHEELS {
        let wheel = Wheel::new(level);
        for i in level..Wheel::SLOTS {
            let ticks = i * usize::pow(Wheel::SLOTS, level as u32);
            let slot_index = wheel.slot_index(ticks as u64);
            assert_eq!(i, slot_index, "wheels[{level}].slot_index({ticks}) == {i}")
        }
    }
}

#[test]
// This test doesn't exercise anything that's potentially memory-unsafe, so
// don't spend time running it under miri.
#[cfg_attr(miri, ignore)]
fn test_next_set_bit() {
    assert_eq!(dbg!(next_set_bit(0b0000_1001, 2)), Some(3));
    assert_eq!(dbg!(next_set_bit(0b0000_1001, 3)), Some(3));
    assert_eq!(dbg!(next_set_bit(0b0000_1001, 0)), Some(0));
    assert_eq!(dbg!(next_set_bit(0b0000_1001, 4)), (Some(64)));
    assert_eq!(dbg!(next_set_bit(0b0000_0000, 0)), None);
    assert_eq!(dbg!(next_set_bit(0b0000_1000, 3)), Some(3));
    assert_eq!(dbg!(next_set_bit(0b0000_1000, 2)), Some(3));
    assert_eq!(dbg!(next_set_bit(0b0000_1000, 4)), Some(64 + 3));
}

proptest! {
    #[test]
    // This test doesn't exercise anything that's potentially memory-unsafe, so
    // don't spend time running it under miri.
    #[cfg_attr(miri, ignore)]
    fn next_set_bit_works(bitmap: u64, offset in 0..64u32) {
        println!("   bitmap: {bitmap:064b}");
        println!("   offset: {offset}");
        // find the next set bit the slow way.
        let mut expected = None;
        for distance in offset..=(offset + u64::BITS) {
            let shift = distance % u64::BITS;
            let bit = bitmap & (1 << shift);

            if bit > 0 {
                // found a set bit, return its distance!
                expected = Some(distance as usize);
                break;
            }
        }

        println!(" expected: {expected:?}");
        prop_assert_eq!(next_set_bit(bitmap, offset), expected);
        println!("       ... ok!\n");
    }
}
