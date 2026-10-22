//! How sim and connection state reads.

use gz_bevy::connection::SimStats;

use crate::format;
use crate::theme::ColorRole;

fn stats(paused: bool) -> SimStats {
    SimStats {
        paused,
        sim_time: 134.38,
        real_time: 140.0,
        real_time_factor: 0.98,
        iterations: 134_380,
        step_size: 0.001,
    }
}

#[test]
fn sim_and_connection_state_reads() {
    assert_eq!(format::clock(134.38), "00:02:14.380");
    assert_eq!(format::clock(0.0), "00:00:00.000");
    assert_eq!(format::clock(-1.0), "00:00:00.000");
    assert_eq!(format::clock(3600.0 * 101.0 + 0.0005), "101:00:00.001");
    assert_eq!(format::clock(59.9996), "00:01:00.000");
    let rtf = |r: f64, paused| {
        format::rtf(Some(&SimStats {
            real_time_factor: r,
            ..stats(paused)
        }))
    };
    assert_eq!(rtf(0.98, false), ("RTF 98%".into(), ColorRole::Ok));
    assert_eq!(rtf(0.9, false), ("RTF 90%".into(), ColorRole::Ok));
    assert_eq!(rtf(0.899, false).1, ColorRole::Warn);
    assert_eq!(rtf(0.5, false).1, ColorRole::Warn);
    assert_eq!(rtf(0.49, false), ("RTF 49%".into(), ColorRole::Danger));
    assert_eq!(rtf(0.98, true), ("paused".into(), ColorRole::Disabled));
    assert_eq!(format::rtf(None), ("RTF —".into(), ColorRole::Disabled));
    assert_eq!(format::thousands(0), "0");
    assert_eq!(format::thousands(999), "999");
    assert_eq!(format::thousands(1000), "1,000");
    assert_eq!(format::thousands(1_234_567), "1,234,567");
    assert_eq!(format::iterations(None), "iter —");
    assert_eq!(format::step(0.001), "1 ms");
    assert_eq!(format::step(0.0005), "0.5 ms");
    assert_eq!(format::step(2.0), "2 s");
    assert_eq!(format::physics(None, None), "—");
    assert_eq!(format::physics(None, Some(0.0)), "—");
    assert_eq!(format::physics(Some("dart"), Some(0.001)), "dart · 1 ms");
    assert_eq!(format::physics(Some("dart"), None), "dart · —");
    assert_eq!(format::fps(Some(59.6)), "60 fps");
    assert_eq!(format::fps(None), "— fps");
    assert_eq!(format::address("ws://localhost:11345/"), "localhost:11345");
    assert_eq!(format::address("localhost:9002"), "localhost:9002");
    assert_eq!(
        format::cursor(Some([2.414, -0.731])),
        "cursor 2.41, \u{2212}0.73"
    );
    assert_eq!(format::cursor(Some([-0.001, 0.0])), "cursor 0.00, 0.00");
    assert_eq!(format::cursor(None), "cursor —");
}

#[test]
fn selection_names_one_and_counts_more() {
    assert_eq!(format::selection(0, None), "sel: —");
    assert_eq!(format::selection(1, Some("amr_01")), "sel: amr_01");
    assert_eq!(format::selection(3, Some("amr_01")), "3 selected");
    assert!(format::rtf_detail(Some(&stats(false))).contains("134,380 iterations · step 1 ms"));
    assert!(format::rtf_detail(Some(&SimStats {
        step_size: 0.0,
        ..stats(true)
    }))
    .ends_with("step —"));
    assert_eq!(format::rtf_detail(None), "No statistics yet");
}

#[test]
fn inspector_values_read_as_the_spec_writes_them() {
    assert_eq!(format::metres(1.2), "1.200 m");
    assert_eq!(format::metres(-0.05), "\u{2212}0.050 m");
    assert_eq!(format::metres(-0.0001), "0.000 m");
    assert_eq!(
        format::degrees(-std::f32::consts::FRAC_PI_2),
        "\u{2212}90.0°"
    );
    assert_eq!(format::number(0.25, 3), "0.25");
    assert_eq!(format::number(10.0, 3), "10");
    assert_eq!(format::number(-1.5, 3), "\u{2212}1.5");
    assert_eq!(format::number(-0.0001, 3), "0");
    assert_eq!(format::yes_no(true), "Yes");
    assert_eq!(format::hex([1.0, 0.5, 0.0]), format!("#{}", "FF8000"));
}
