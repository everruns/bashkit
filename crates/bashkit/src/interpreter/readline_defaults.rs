//! Readline's default emacs keymap, function names and variables, as GNU
//! bash 5.2 reports them (`bind -l`, `bind -p`, `bind -v`). Data only.

/// `bind -l`: every readline function name, sorted.
pub(crate) const FUNCTION_NAMES: &[&str] = &[
    "abort",
    "accept-line",
    "alias-expand-line",
    "arrow-key-prefix",
    "backward-byte",
    "backward-char",
    "backward-delete-char",
    "backward-kill-line",
    "backward-kill-word",
    "backward-word",
    "beginning-of-history",
    "beginning-of-line",
    "bracketed-paste-begin",
    "call-last-kbd-macro",
    "capitalize-word",
    "character-search",
    "character-search-backward",
    "clear-display",
    "clear-screen",
    "complete",
    "complete-command",
    "complete-filename",
    "complete-hostname",
    "complete-into-braces",
    "complete-username",
    "complete-variable",
    "copy-backward-word",
    "copy-forward-word",
    "copy-region-as-kill",
    "dabbrev-expand",
    "delete-char",
    "delete-char-or-list",
    "delete-horizontal-space",
    "digit-argument",
    "display-shell-version",
    "do-lowercase-version",
    "downcase-word",
    "dump-functions",
    "dump-macros",
    "dump-variables",
    "dynamic-complete-history",
    "edit-and-execute-command",
    "emacs-editing-mode",
    "end-kbd-macro",
    "end-of-history",
    "end-of-line",
    "exchange-point-and-mark",
    "fetch-history",
    "forward-backward-delete-char",
    "forward-byte",
    "forward-char",
    "forward-search-history",
    "forward-word",
    "glob-complete-word",
    "glob-expand-word",
    "glob-list-expansions",
    "history-and-alias-expand-line",
    "history-expand-line",
    "history-search-backward",
    "history-search-forward",
    "history-substring-search-backward",
    "history-substring-search-forward",
    "insert-comment",
    "insert-completions",
    "insert-last-argument",
    "kill-line",
    "kill-region",
    "kill-whole-line",
    "kill-word",
    "magic-space",
    "menu-complete",
    "menu-complete-backward",
    "next-history",
    "next-screen-line",
    "non-incremental-forward-search-history",
    "non-incremental-forward-search-history-again",
    "non-incremental-reverse-search-history",
    "non-incremental-reverse-search-history-again",
    "old-menu-complete",
    "operate-and-get-next",
    "overwrite-mode",
    "possible-command-completions",
    "possible-completions",
    "possible-filename-completions",
    "possible-hostname-completions",
    "possible-username-completions",
    "possible-variable-completions",
    "previous-history",
    "previous-screen-line",
    "print-last-kbd-macro",
    "quoted-insert",
    "re-read-init-file",
    "redraw-current-line",
    "reverse-search-history",
    "revert-line",
    "self-insert",
    "set-mark",
    "shell-backward-kill-word",
    "shell-backward-word",
    "shell-expand-line",
    "shell-forward-word",
    "shell-kill-word",
    "shell-transpose-words",
    "skip-csi-sequence",
    "spell-correct-word",
    "start-kbd-macro",
    "tab-insert",
    "tilde-expand",
    "transpose-chars",
    "transpose-words",
    "tty-status",
    "undo",
    "universal-argument",
    "unix-filename-rubout",
    "unix-line-discard",
    "unix-word-rubout",
    "upcase-word",
    "vi-append-eol",
    "vi-append-mode",
    "vi-arg-digit",
    "vi-bWord",
    "vi-back-to-indent",
    "vi-backward-bigword",
    "vi-backward-word",
    "vi-bword",
    "vi-change-case",
    "vi-change-char",
    "vi-change-to",
    "vi-char-search",
    "vi-column",
    "vi-complete",
    "vi-delete",
    "vi-delete-to",
    "vi-eWord",
    "vi-edit-and-execute-command",
    "vi-editing-mode",
    "vi-end-bigword",
    "vi-end-word",
    "vi-eof-maybe",
    "vi-eword",
    "vi-fWord",
    "vi-fetch-history",
    "vi-first-print",
    "vi-forward-bigword",
    "vi-forward-word",
    "vi-fword",
    "vi-goto-mark",
    "vi-insert-beg",
    "vi-insertion-mode",
    "vi-match",
    "vi-movement-mode",
    "vi-next-word",
    "vi-overstrike",
    "vi-overstrike-delete",
    "vi-prev-word",
    "vi-put",
    "vi-redo",
    "vi-replace",
    "vi-rubout",
    "vi-search",
    "vi-search-again",
    "vi-set-mark",
    "vi-subst",
    "vi-tilde-expand",
    "vi-undo",
    "vi-unix-word-rubout",
    "vi-yank-arg",
    "vi-yank-pop",
    "vi-yank-to",
    "yank",
    "yank-last-arg",
    "yank-nth-arg",
    "yank-pop",
];

/// `bind -p` for the default emacs keymap: `"keyseq": function` lines,
/// functions sorted, each function's key sequences in key order.
pub(crate) const DEFAULT_EMACS_BINDINGS: &str = r##""\C-g": abort
"\C-x\C-g": abort
"\M-\C-g": abort
"\C-j": accept-line
"\C-m": accept-line
"\C-b": backward-char
"\M-OD": backward-char
"\M-[D": backward-char
"\C-h": backward-delete-char
"\C-?": backward-delete-char
"\C-x\C-?": backward-kill-line
"\M-\C-h": backward-kill-word
"\M-\C-?": backward-kill-word
"\M-\M-[D": backward-word
"\M-[1;3D": backward-word
"\M-[1;5D": backward-word
"\M-[5D": backward-word
"\M-b": backward-word
"\M-<": beginning-of-history
"\C-a": beginning-of-line
"\M-OH": beginning-of-line
"\M-[1~": beginning-of-line
"\M-[H": beginning-of-line
"\M-[200~": bracketed-paste-begin
"\C-xe": call-last-kbd-macro
"\M-c": capitalize-word
"\C-]": character-search
"\M-\C-]": character-search-backward
"\M-\C-l": clear-display
"\C-l": clear-screen
"\C-i": complete
"\M-\M-\000": complete
"\M-!": complete-command
"\M-/": complete-filename
"\M-@": complete-hostname
"\M-{": complete-into-braces
"\M-~": complete-username
"\M-$": complete-variable
"\C-d": delete-char
"\M-[3~": delete-char
"\M-\\": delete-horizontal-space
"\M--": digit-argument
"\M-0": digit-argument
"\M-1": digit-argument
"\M-2": digit-argument
"\M-3": digit-argument
"\M-4": digit-argument
"\M-5": digit-argument
"\M-6": digit-argument
"\M-7": digit-argument
"\M-8": digit-argument
"\M-9": digit-argument
"\C-x\C-v": display-shell-version
"\C-xA": do-lowercase-version
"\C-xB": do-lowercase-version
"\C-xC": do-lowercase-version
"\C-xD": do-lowercase-version
"\C-xE": do-lowercase-version
"\C-xF": do-lowercase-version
"\C-xG": do-lowercase-version
"\C-xH": do-lowercase-version
"\C-xI": do-lowercase-version
"\C-xJ": do-lowercase-version
"\C-xK": do-lowercase-version
"\C-xL": do-lowercase-version
"\C-xM": do-lowercase-version
"\C-xN": do-lowercase-version
"\C-xO": do-lowercase-version
"\C-xP": do-lowercase-version
"\C-xQ": do-lowercase-version
"\C-xR": do-lowercase-version
"\C-xS": do-lowercase-version
"\C-xT": do-lowercase-version
"\C-xU": do-lowercase-version
"\C-xV": do-lowercase-version
"\C-xW": do-lowercase-version
"\C-xX": do-lowercase-version
"\C-xY": do-lowercase-version
"\C-xZ": do-lowercase-version
"\M-A": do-lowercase-version
"\M-B": do-lowercase-version
"\M-C": do-lowercase-version
"\M-D": do-lowercase-version
"\M-E": do-lowercase-version
"\M-F": do-lowercase-version
"\M-G": do-lowercase-version
"\M-H": do-lowercase-version
"\M-I": do-lowercase-version
"\M-J": do-lowercase-version
"\M-K": do-lowercase-version
"\M-L": do-lowercase-version
"\M-M": do-lowercase-version
"\M-N": do-lowercase-version
"\M-P": do-lowercase-version
"\M-Q": do-lowercase-version
"\M-R": do-lowercase-version
"\M-S": do-lowercase-version
"\M-T": do-lowercase-version
"\M-U": do-lowercase-version
"\M-V": do-lowercase-version
"\M-W": do-lowercase-version
"\M-X": do-lowercase-version
"\M-Y": do-lowercase-version
"\M-Z": do-lowercase-version
"\M-l": downcase-word
"\M-\C-i": dynamic-complete-history
"\C-x\C-e": edit-and-execute-command
"\C-x)": end-kbd-macro
"\M->": end-of-history
"\C-e": end-of-line
"\M-OF": end-of-line
"\M-[4~": end-of-line
"\M-[F": end-of-line
"\C-x\C-x": exchange-point-and-mark
"\C-f": forward-char
"\M-OC": forward-char
"\M-[C": forward-char
"\C-s": forward-search-history
"\M-\M-[C": forward-word
"\M-[1;3C": forward-word
"\M-[1;5C": forward-word
"\M-[5C": forward-word
"\M-f": forward-word
"\M-g": glob-complete-word
"\C-x*": glob-expand-word
"\C-xg": glob-list-expansions
"\M-^": history-expand-line
"\M-[5~": history-search-backward
"\M-[6~": history-search-forward
"\M-#": insert-comment
"\M-*": insert-completions
"\M-.": insert-last-argument
"\M-_": insert-last-argument
"\C-k": kill-line
"\M-[3;5~": kill-word
"\M-d": kill-word
"\C-n": next-history
"\M-OB": next-history
"\M-[B": next-history
"\M-n": non-incremental-forward-search-history
"\M-p": non-incremental-reverse-search-history
"\C-o": operate-and-get-next
"\C-x!": possible-command-completions
"\M-=": possible-completions
"\M-?": possible-completions
"\C-x/": possible-filename-completions
"\C-x@": possible-hostname-completions
"\C-x~": possible-username-completions
"\C-x$": possible-variable-completions
"\C-p": previous-history
"\M-OA": previous-history
"\M-[A": previous-history
"\C-q": quoted-insert
"\C-v": quoted-insert
"\M-[2~": quoted-insert
"\C-x\C-r": re-read-init-file
"\C-r": reverse-search-history
"\M-\C-r": revert-line
"\M-r": revert-line
" ": self-insert
"!": self-insert
"\"": self-insert
"#": self-insert
"$": self-insert
"%": self-insert
"&": self-insert
"'": self-insert
"(": self-insert
")": self-insert
"*": self-insert
"+": self-insert
",": self-insert
"-": self-insert
".": self-insert
"/": self-insert
"0": self-insert
"1": self-insert
"2": self-insert
"3": self-insert
"4": self-insert
"5": self-insert
"6": self-insert
"7": self-insert
"8": self-insert
"9": self-insert
":": self-insert
";": self-insert
"<": self-insert
"=": self-insert
">": self-insert
"?": self-insert
"@": self-insert
"A": self-insert
"B": self-insert
"C": self-insert
"D": self-insert
"E": self-insert
"F": self-insert
"G": self-insert
"H": self-insert
"I": self-insert
"J": self-insert
"K": self-insert
"L": self-insert
"M": self-insert
"N": self-insert
"O": self-insert
"P": self-insert
"Q": self-insert
"R": self-insert
"S": self-insert
"T": self-insert
"U": self-insert
"V": self-insert
"W": self-insert
"X": self-insert
"Y": self-insert
"Z": self-insert
"[": self-insert
"\\": self-insert
"]": self-insert
"^": self-insert
"_": self-insert
"`": self-insert
"a": self-insert
"b": self-insert
"c": self-insert
"d": self-insert
"e": self-insert
"f": self-insert
"g": self-insert
"h": self-insert
"i": self-insert
"j": self-insert
"k": self-insert
"l": self-insert
"m": self-insert
"n": self-insert
"o": self-insert
"p": self-insert
"q": self-insert
"r": self-insert
"s": self-insert
"t": self-insert
"u": self-insert
"v": self-insert
"w": self-insert
"x": self-insert
"y": self-insert
"z": self-insert
"{": self-insert
"|": self-insert
"}": self-insert
"~": self-insert
"\200": self-insert
"\201": self-insert
"\202": self-insert
"\203": self-insert
"\204": self-insert
"\205": self-insert
"\206": self-insert
"\207": self-insert
"\210": self-insert
"\211": self-insert
"\212": self-insert
"\213": self-insert
"\214": self-insert
"\215": self-insert
"\216": self-insert
"\217": self-insert
"\220": self-insert
"\221": self-insert
"\222": self-insert
"\223": self-insert
"\224": self-insert
"\225": self-insert
"\226": self-insert
"\227": self-insert
"\230": self-insert
"\231": self-insert
"\232": self-insert
"\233": self-insert
"\234": self-insert
"\235": self-insert
"\236": self-insert
"\237": self-insert
"\240": self-insert
"\241": self-insert
"\242": self-insert
"\243": self-insert
"\244": self-insert
"\245": self-insert
"\246": self-insert
"\247": self-insert
"\250": self-insert
"\251": self-insert
"\252": self-insert
"\253": self-insert
"\254": self-insert
"\255": self-insert
"\256": self-insert
"\257": self-insert
"\260": self-insert
"\261": self-insert
"\262": self-insert
"\263": self-insert
"\264": self-insert
"\265": self-insert
"\266": self-insert
"\267": self-insert
"\270": self-insert
"\271": self-insert
"\272": self-insert
"\273": self-insert
"\274": self-insert
"\275": self-insert
"\276": self-insert
"\277": self-insert
"\300": self-insert
"\301": self-insert
"\302": self-insert
"\303": self-insert
"\304": self-insert
"\305": self-insert
"\306": self-insert
"\307": self-insert
"\310": self-insert
"\311": self-insert
"\312": self-insert
"\313": self-insert
"\314": self-insert
"\315": self-insert
"\316": self-insert
"\317": self-insert
"\320": self-insert
"\321": self-insert
"\322": self-insert
"\323": self-insert
"\324": self-insert
"\325": self-insert
"\326": self-insert
"\327": self-insert
"\330": self-insert
"\331": self-insert
"\332": self-insert
"\333": self-insert
"\334": self-insert
"\335": self-insert
"\336": self-insert
"\337": self-insert
"\340": self-insert
"\341": self-insert
"\342": self-insert
"\343": self-insert
"\344": self-insert
"\345": self-insert
"\346": self-insert
"\347": self-insert
"\350": self-insert
"\351": self-insert
"\352": self-insert
"\353": self-insert
"\354": self-insert
"\355": self-insert
"\356": self-insert
"\357": self-insert
"\360": self-insert
"\361": self-insert
"\362": self-insert
"\363": self-insert
"\364": self-insert
"\365": self-insert
"\366": self-insert
"\367": self-insert
"\370": self-insert
"\371": self-insert
"\372": self-insert
"\373": self-insert
"\374": self-insert
"\375": self-insert
"\376": self-insert
"\377": self-insert
"\C-@": set-mark
"\M- ": set-mark
"\M-\C-b": shell-backward-word
"\M-\C-e": shell-expand-line
"\M-\C-f": shell-forward-word
"\M-\C-d": shell-kill-word
"\M-\C-t": shell-transpose-words
"\C-xs": spell-correct-word
"\C-x(": start-kbd-macro
"\M-&": tilde-expand
"\C-t": transpose-chars
"\M-t": transpose-words
"\C-x\C-u": undo
"\C-_": undo
"\C-u": unix-line-discard
"\C-w": unix-word-rubout
"\M-u": upcase-word
"\C-y": yank
"\M-.": yank-last-arg
"\M-_": yank-last-arg
"\M-\C-y": yank-nth-arg
"\M-y": yank-pop"##;

/// `bind -v`: (name, default value), booleans first, each group sorted.
pub(crate) const VARIABLES: &[(&str, &str)] = &[
    ("bind-tty-special-chars", "on"),
    ("blink-matching-paren", "off"),
    ("byte-oriented", "off"),
    ("colored-completion-prefix", "off"),
    ("colored-stats", "off"),
    ("completion-ignore-case", "off"),
    ("completion-map-case", "off"),
    ("convert-meta", "on"),
    ("disable-completion", "off"),
    ("echo-control-characters", "on"),
    ("enable-active-region", "on"),
    ("enable-bracketed-paste", "on"),
    ("enable-keypad", "off"),
    ("enable-meta-key", "on"),
    ("expand-tilde", "off"),
    ("history-preserve-point", "off"),
    ("horizontal-scroll-mode", "off"),
    ("input-meta", "on"),
    ("mark-directories", "on"),
    ("mark-modified-lines", "off"),
    ("mark-symlinked-directories", "off"),
    ("match-hidden-files", "on"),
    ("menu-complete-display-prefix", "off"),
    ("meta-flag", "on"),
    ("output-meta", "on"),
    ("page-completions", "on"),
    ("prefer-visible-bell", "on"),
    ("print-completions-horizontally", "off"),
    ("revert-all-at-newline", "off"),
    ("show-all-if-ambiguous", "off"),
    ("show-all-if-unmodified", "off"),
    ("show-mode-in-prompt", "off"),
    ("skip-completed-text", "off"),
    ("visible-stats", "off"),
    ("bell-style", "audible"),
    ("comment-begin", "#"),
    ("completion-display-width", "-1"),
    ("completion-prefix-display-length", "0"),
    ("completion-query-items", "100"),
    ("editing-mode", "emacs"),
    ("emacs-mode-string", "@"),
    ("history-size", "0"),
    ("keymap", "emacs"),
    ("keyseq-timeout", "500"),
    ("vi-cmd-mode-string", "(cmd)"),
    ("vi-ins-mode-string", "(ins)"),
];
