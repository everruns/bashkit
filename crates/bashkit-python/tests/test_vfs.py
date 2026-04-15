"""Virtual filesystem, mount, and direct VFS API tests."""

from . import _bashkit_categories as _categories

_NAMES = (
    "test_bash_fs_handle_bytes_roundtrip",
    "test_bash_files_dict",
    "test_bash_files_dict_callables_are_lazy_and_cached",
    "test_bash_files_dict_callable_errors_and_invalid_returns_raise",
    "test_bash_mounts_readonly_by_default",
    "test_bash_mounts_writable",
    "test_bash_live_mount_preserves_state_and_unmounts",
    "test_bash_fs_handle_tracks_reset_and_new_live_mounts",
    "test_bash_fs_handle_supports_directory_ops_and_links",
    "test_bash_direct_vfs_methods_cover_core_ops",
    "test_bash_direct_vfs_methods_raise_on_missing_paths_and_non_utf8",
    "test_bash_direct_vfs_methods_track_shell_changes_and_reset",
    "test_bash_unmount_nonexistent_raises",
    "test_bash_mounts_missing_host_path_raises",
    "test_bash_mounts_invalid_entry_raises",
    "test_bash_files_mount_has_writable_mode",
    "test_filesystem_real_nonexistent_host_path_raises",
    "test_filesystem_read_nonexistent_file_raises",
    "test_filesystem_stat_nonexistent_raises",
    "test_bashtool_files_dict_callables_are_lazy_and_cached",
    "test_bashtool_realfs_and_fs_handle",
    "test_bashtool_live_mount_preserves_state",
    "test_bashtool_direct_vfs_methods_cover_core_ops",
)

globals().update({name: getattr(_categories, name) for name in _NAMES})

del _categories
del _NAMES
