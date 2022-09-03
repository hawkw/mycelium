use super::*;

#[test]
fn wheel_indices() {
    let core = Core::new();
    for ticks in 0..64 {
        assert_eq!(
            core.wheel_index(ticks),
            0,
            "Core::wheel_index({ticks}) == 0"
        )
    }

    for wheel in 1..Core::WHEELS as usize {
        for slot in wheel..wheel::SLOTS {
            let ticks = (slot * usize::pow(wheel::SLOTS, wheel as u32)) as u64;
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
