# Helvetica loads its bold face when asked for regular

On macOS (GitHub CI, 2026-10-19) two unit tests panic in `load_face` in `markdown-notes/src/crates/markdown-notes-plugin/src/fonts.rs`: `fonts::tests::bold_is_a_different_face_from_the_regular_one` and `gui::tests::the_caret_lands_after_bold_text_not_inside_it`. The message is `the font Helvetica has no face of weight 400, only 700`. Windows (Segoe UI, Consolas) passes.

`load_face` calls font-kit's `SystemSource::select_best_match` with `FamilyName::Title("Helvetica")` and the requested weight, loads the returned handle, and compares `font.properties().weight` with the request. The handle's `font_index` is also what egui's `FontData::index` is set to.

Helvetica on macOS is a collection file. Not yet known which of these is true: font-kit matched the bold face, or it matched the regular face and loading the handle by its `font_index` gives a different face from the collection. In the second case egui would also draw the wrong face, because it is given the same index.

**To do on a Mac:** write a named test that runs `load_face`'s steps for Helvetica at 400 and at 700 and reports the handle's path and `font_index`, and the loaded face's PostScript name and weight. Run it with `scripts/test.sh markdown-notes --only <name>`. Choose the fix from what it reports.

**Candidate fix, not tried:** use font-kit only to find the family's files (`select_family_by_name`), read those files into a `fontdb::Database` with `load_font_file`, pick the face by family name and weight with `Database::query`, and take the bytes and face index from `with_face_data`. `system_fonts` in `diagram.rs` already reads the diagram font's files this way, so `load_face` and `system_fonts` could share one function.
