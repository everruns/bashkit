### grep_bre_literal_paren
# Bug: grep 'feat(' should match literal parenthesis — in BRE, ( is literal
# Only \( starts a group in BRE. Bashkit incorrectly treated ( as a group metachar.
# Affected eval tasks: complex_release_notes (fails 3/4 models)
# Root cause: regex engine didn't distinguish BRE vs ERE metachar rules
printf 'feat(auth): add OAuth2\nfix(api): handle null\nchore: update\n' | grep 'feat('
### expect
feat(auth): add OAuth2
### end

### grep_bre_literal_paren_pattern
# Generalized: filtering conventional commit lines by type prefix with parens
printf 'feat(auth): OAuth2\nfeat(ui): dark mode\nfix(api): null body\n' | grep '^feat('
### expect
feat(auth): OAuth2
feat(ui): dark mode
### end
