- [x] Markdown Notes: the text area does not wrap lines. Make sure it does this, and that it reflows on window resize.
- [x] MacOS mini host can't open VST3 using the file browse dialog, greyes out the VST3 bundle.
- [x] a test saves two files ("...-one.md" and "...-two.md") and then does nothign with the first file, so that's a completely meaningless action.
- [x] that same test then does not check BOTH tabs to confirm they contain the content they should.
- [x] tests wait too long between actions
- [x] test should provably test what they were designed for, not "verifying a screnshot was written" when the point of the screenshot was to verify that what was on screen was what was supposed to be on screen. Any test that doesn't ACTUALLY verify the content of screenshots should be fixed to actually do what the test is meant to test.
- [x] Text selection is naff: I can't just "select all text" using normal click dragging. I can't even do that in markdown source view, wtf?
- [x] Clicking in the markdown notes text area without any text to position a cursor on should simply "remove the cursor" (if there already is one somewhere) or "not place a cursor" (if there isn't one yet). This should not affect cursorplacement on load: the cursor should be at the end of every tab by default so users can immediately resume their work in that tab. Note that if someone clicks next to a line of text, that DOES have content, and the cursor should be placed at the start or end of that line (depending on whether the user clicked left or right of it)

- [ ] Check whether mini-host can load VST 1 and 2 (i.e. is it backwards compatible). If not, add that if it's not too much work.
