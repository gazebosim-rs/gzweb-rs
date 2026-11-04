// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
use super::*;
use bevy::ecs::system::RunSystemOnce;
use gz_core::primitive::lifecycle::{Model, Template};
fn catalog() -> Catalog {
    Catalog {
        version: 1,
        world_id: 1,
        epoch: 0,
        scene_sequence: 1,
        templates: vec![Template {
            template_id: 40,
            label: "Local mesh".into(),
        }],
        models: vec![
            Model {
                model_id: 20,
                name: "same".into(),
                visual_ids: vec![30],
                can_remove: true,
                can_set_state: true,
            },
            Model {
                model_id: 21,
                name: "same".into(),
                visual_ids: vec![31],
                can_remove: false,
                can_set_state: false,
            },
        ],
    }
}
#[test]
fn public_model_selection_rebinds_actual_visual_entities_and_retires_on_epoch_or_capability_loss() {
    let mut app = App::new();
    let first = app.world_mut().spawn_empty().id();
    let second = app.world_mut().spawn_empty().id();
    let mut index = GzIndex::default();
    index.by_id.insert(30, first);
    index.by_id.insert(31, second);
    let mut status = GzStatus::new("ws://test");
    status.lifecycle = Some(catalog());
    app.insert_resource(index)
        .insert_resource(status)
        .init_resource::<Selection>()
        .init_resource::<Editor>();
    app.world_mut().run_system_once(sync).unwrap();
    app.world_mut().resource_mut::<Editor>().selected = Some(20);
    app.world_mut().run_system_once(sync).unwrap();
    assert_eq!(app.world().resource::<Selection>().entities, vec![first]);
    // Geometry replacement can retire render entities while server model IDs stay.
    app.world_mut().entity_mut(first).despawn();
    let replacement = app.world_mut().spawn_empty().id();
    app.world_mut()
        .resource_mut::<GzIndex>()
        .by_id
        .insert(30, replacement);
    app.world_mut().run_system_once(sync).unwrap();
    assert_eq!(
        app.world().resource::<Selection>().entities,
        vec![replacement]
    );
    assert_eq!(app.world().resource::<Editor>().selected, Some(20));
    app.world_mut()
        .resource_mut::<GzStatus>()
        .lifecycle
        .as_mut()
        .unwrap()
        .models
        .retain(|m| m.model_id != 20);
    app.world_mut().run_system_once(sync).unwrap();
    assert!(app.world().resource::<Selection>().is_empty());
    assert!(app.world().resource::<Editor>().selected.is_none());
    app.world_mut().resource_mut::<Editor>().selected = Some(21);
    app.world_mut()
        .resource_mut::<GzStatus>()
        .lifecycle
        .as_mut()
        .unwrap()
        .epoch = 1;
    app.world_mut().run_system_once(sync).unwrap();
    assert!(app.world().resource::<Editor>().selected.is_none());
    assert!(app.world().resource::<Selection>().is_empty());
    app.world_mut().resource_mut::<GzStatus>().lifecycle = None;
    app.world_mut().run_system_once(sync).unwrap();
    assert!(app.world().resource::<Editor>().catalog.is_none());
}
#[test]
fn explicit_editor_commands_use_catalog_models_and_world_origin_state_not_visual_ids_or_names() {
    let mut editor = Editor {
        catalog: Some(catalog()),
        selected: Some(20),
        read_for: Some(20),
        template: Some(40),
        ..default()
    };
    editor.state = State {
        translation: [1., 2., 3.],
        rotation: [0., 0., 1., 0.],
        linear_velocity: [4., 5., 6.],
        angular_velocity: [7., 8., 9.],
    };
    assert_eq!(
        editor.command(&Operation::Read).unwrap(),
        Edit::GetState { model_id: 20 }
    );
    assert_eq!(
        editor.command(&Operation::Remove).unwrap(),
        Edit::Remove { model_id: 20 }
    );
    assert_eq!(
        editor.command(&Operation::Apply).unwrap(),
        Edit::SetState {
            model_id: 20,
            state: editor.state.clone()
        }
    );
    assert_eq!(
        editor.command(&Operation::Spawn).unwrap(),
        Edit::Spawn {
            template_id: 40,
            translation: [1., 2., 3.],
            rotation: [0., 0., 1., 0.]
        }
    );
    editor.selected = Some(21);
    assert!(
        editor.command(&Operation::Apply).is_err(),
        "a new selection must be read before Apply"
    );
    editor.read_for = Some(21);
    assert!(editor
        .command(&Operation::Apply)
        .unwrap()
        .validate(editor.catalog.as_ref().unwrap())
        .is_err());
    editor.selected = None;
    assert!(editor.command(&Operation::Read).is_err());
    editor.template = None;
    assert!(editor.command(&Operation::Spawn).is_err());
}
#[test]
fn only_explicit_successful_selected_state_reply_updates_editor_and_disconnect_clears_it() {
    let mut app = App::new();
    let mut status = GzStatus::new("ws://test");
    status.lifecycle = Some(catalog());
    app.insert_resource(status)
        .init_resource::<GzIndex>()
        .init_resource::<Selection>()
        .init_resource::<Editor>();
    app.world_mut().run_system_once(sync).unwrap();
    app.world_mut().resource_mut::<Editor>().selected = Some(20);
    let state = State {
        translation: [5., 6., 7.],
        ..State::default()
    };
    let reply = Reply {
        version: 1,
        request_id: 1,
        action: gz_core::primitive::lifecycle::Action::GetState,
        world_id: 1,
        epoch: 0,
        scene_sequence: 1,
        seconds: 0,
        nanoseconds: 0,
        paused: true,
        ok: true,
        error: None,
        model_id: Some(20),
        state: Some(state.clone()),
    };
    app.world_mut().resource_mut::<GzStatus>().lifecycle_reply = Some(reply);
    app.world_mut().run_system_once(sync).unwrap();
    assert_eq!(app.world().resource::<Editor>().state, state);
    app.world_mut().resource_mut::<GzStatus>().lifecycle = None;
    app.world_mut().resource_mut::<GzStatus>().lifecycle_reply = None;
    app.world_mut().run_system_once(sync).unwrap();
    assert_eq!(app.world().resource::<Editor>().state, State::default());
}
