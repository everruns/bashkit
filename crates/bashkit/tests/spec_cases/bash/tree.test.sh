### tree_noreport
### bash_diff: tree --noreport is bashkit builtin
# tree --noreport should suppress the report line
mkdir -p /tmp/tree_nr/a
touch /tmp/tree_nr/a/f.txt
tree --noreport /tmp/tree_nr
### expect
/tmp/tree_nr
└── a
    └── f.txt
### end
