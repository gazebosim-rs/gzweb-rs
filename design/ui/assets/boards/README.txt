Gazebo UI — W2 design boards, exported 2026-10-19 at 2x (Retina) resolution.

light/  and  dark/  hold the same five boards in each theme:
  01-main-screen              full app, split viewport
  02-menus-and-status-bar     world menu, camera/overlay/layout menus, message tray, status states
                              (its "viewport floating controls" column is marked obsolete; see 03)
  03-viewport-camera-controls view cube, rail, menus, input map, split behavior
  04-activity-panels          all eight sidebar panels
  05-inspectors               world, model, link, joint, light, sensor
06-theme-tokens-light-vs-dark.png  components and color tokens side by side

01-main-screen-dark was designed by hand. The other dark boards were converted from the
light boards with the token map in tokens.json, so treat small color details as indicative.
The spec (DESIGN.md, tokens.json) wins where an image differs.
