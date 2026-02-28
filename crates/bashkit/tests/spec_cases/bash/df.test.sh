### df_shows_vfs
### bash_diff: VFS shows virtual filesystem name
# df output includes bashkit-vfs
df | grep -q bashkit-vfs && echo "ok"
### expect
ok
### end

### df_human_readable
### bash_diff: VFS shows virtual filesystem stats
# df -h includes human-readable header
df -h | head -1 | grep -q "Size" && echo "ok"
### expect
ok
### end

### df_has_header
### bash_diff: VFS shows virtual filesystem stats
# df shows filesystem header
df | head -1 | grep -q "Filesystem" && echo "ok"
### expect
ok
### end
