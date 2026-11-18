# Working rules

Read `docs/PROJECT_MAP.md` for the affected entry point and `docs/SHADER_INDEX.md` for shader roles. Search the pass headers and relevant functions before opening the full Kerr–Newman source.

For a native or browser geometry regression, start with `tests/fixtures/centering.wgsl`. Check physical target size, texture origin, one uniform buffer per pass, same-frame channel order and previous-frame feedback. Validate a source edit or backend change with a preview-only capture. Compare the original Kerr–Newman with its fixed reference after the small fixture passes.

Work on compilation and graph behavior in `shader-core/`. Keep native UI and Web APIs in their respective hosts. Give local files the same validation, source inspection and error recovery as gallery items. Measure the rendered frames and report unavailable quantities explicitly.

The live site is built from the repository root using `apps/web/Dockerfile`. Keep the release manual and local source processing in the browser. Update `AGENTS.md` and `CLAUDE.md` together when the product or build contract changes.
