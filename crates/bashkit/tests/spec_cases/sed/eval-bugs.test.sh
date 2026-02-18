### sed_capture_group_complex_bre
# Bug: sed 's/^[a-z]*(\([^)]*\)): \(.*\)/- \1: \2/' silently produces no change
# Simple capture group swap works (see sed_regex_group), but multi-group extraction
#   from complex patterns with literal chars between groups failed
# Affected eval tasks: complex_release_notes (fails 3/4 models)
# Root cause: BRE-to-ERE conversion didn't escape literal ( ) in BRE patterns
printf 'feat(auth): add OAuth2\n' | sed 's/^[a-z]*(\([^)]*\)): \(.*\)/- \1: \2/'
### expect
- auth: add OAuth2
### end

### sed_ere_capture_group_extract
# Same class of bug with ERE syntax: ( ) are group metachars in -E mode
# Pattern: extract scope and description from conventional commit format
# Affected eval tasks: complex_release_notes
printf 'fix(api): handle null response\n' | sed -E 's/^[a-z]+\(([^)]+)\): (.*)/- \1: \2/'
### expect
- api: handle null response
### end
