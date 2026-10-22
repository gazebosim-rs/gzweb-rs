use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::inspectors::common::{fixture, EntityAction};
use crate::inspectors::model::tests::{action, select};
use crate::test_support::frames;
use crate::test_support::shell::*;

#[test]
fn a_light_shows_its_parameters_and_hides() {
    let mut app = shell_app((1440, 900), 1.0);
    frames(&mut app, 2);
    let s = fixture::scene(&mut app);
    select(&mut app, vec![s.sun]);
    let text = all_text(&mut app);
    // Diffuse 0.8 and specular 0.2 grey, as hex.
    let (diffuse, specular) = (crate::format::hex([0.8; 3]), crate::format::hex([0.2; 3]));
    for want in [
        "LIGHT",
        "directional",
        &diffuse,
        &specular,
        "0  0  −1",
        "Yes",
    ] {
        assert!(text.contains(want), "{want} in {text}");
    }
    let hide = action(&mut app, EntityAction::Hide);
    app.world_mut().trigger(Activate { entity: hide });
    frames(&mut app, 2);
    assert_eq!(
        app.world().get::<Visibility>(s.sun),
        Some(&Visibility::Hidden)
    );
    app.world_mut().trigger(Activate { entity: hide });
    frames(&mut app, 2);
    assert_eq!(
        app.world().get::<Visibility>(s.sun),
        Some(&Visibility::Inherited)
    );
}
