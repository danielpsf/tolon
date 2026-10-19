# UI agent (ui/, crates/tolon-app commands)

React + TypeScript, styled only with Swiss Moss. The mockups in `mockups/mockups.figcraft` are the spec; export the frame with `figcraft-cli`, measure, match (commands in `docs/decisions.md`).

Rules on top of `AGENTS.md`:
- Components come from Swiss Moss (`sm-btn`, `sm-field`, `sm-tag-input`, `sm-tabs`, `sm-switch`, `sm-alert`, `sm-toast`, `sm-dialog`, and so on). If a component is missing, propose it to the Swiss Moss repo as an issue and use the closest existing one meanwhile.
- All copy in `ui/src/copy.ts`. Follow the writing rules.
- The interrupt page: full-bleed moss fill, display-xl title, countdown every second, focus trap, keys Enter / Esc / D / Space, works at 1280x800 and 3840x2160.
- No business logic: the view calls commands and renders events. If you find yourself computing a schedule in TypeScript, stop and move it to core.
- Light and dark must both match the mockups; test with `data-theme` forced each way.
