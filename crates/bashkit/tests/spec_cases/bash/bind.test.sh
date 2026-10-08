# The bind builtin as a non-interactive bash reports it (no line editing).

### bind_lists_functions
bind -l 2>/dev/null | grep -c '^accept-line$'
bind -l 2>/dev/null | wc -l
### expect
1
173
### end

### bind_print_and_query
bind -p 2>/dev/null | grep vi-subst | sed 's/^# //'
bind -P 2>/dev/null | grep -E '^(vi-subst|yank) '
bind -q yank 2>/dev/null
echo status=$?
bind -q vi-subst 2>/dev/null
echo status=$?
bind -q zz-bad 2>/dev/null
echo status=$?
### expect
vi-subst (not bound)
vi-subst is not bound to any keys
yank can be found on "\C-y".
yank can be invoked via "\C-y".
status=0
vi-subst is not bound to any keys.
status=1
status=1
### end

### bind_variables
bind -v 2>/dev/null | grep blink-matching-paren
bind -V 2>/dev/null | grep blink-matching-paren
bind 'set bell-style none' 2>/dev/null
bind -v 2>/dev/null | grep bell-style
### expect
set blink-matching-paren off
blink-matching-paren is set to `off'
set bell-style none
### end

### bind_key_changes
bind '"\C-o\C-s\C-h": yank' 2>/dev/null
bind -q yank 2>/dev/null
bind -r '\C-o\C-s\C-h' 2>/dev/null
bind -q yank 2>/dev/null
bind -x '"\C-o\C-s\C-h": echo foo' 2>/dev/null
bind -X 2>/dev/null
bind -m vi -x '"\C-o": echo vi' 2>/dev/null
bind -m vi -X 2>/dev/null
bind '"\C-xq": "macro"' 2>/dev/null
bind -s 2>/dev/null
bind -u yank 2>/dev/null
bind -q yank 2>/dev/null
echo status=$?
### expect
yank can be invoked via "\C-o\C-s\C-h", "\C-y".
yank can be invoked via "\C-y".
"\C-o\C-s\C-h": "echo foo"
"\C-o": "echo vi"
"\C-xq": "macro"
yank is not bound to any keys.
status=1
### end

### bind_errors
bind -Z 2>/dev/null
echo status=$?
bind -m bogus -p 2>/dev/null
echo status=$?
bind -f /nonexistent 2>/dev/null
echo status=$?
### expect
status=2
status=1
status=1
### end
