# BashBox security cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_security_strings_arrays_and_braces
# strings, arrays and braces
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=0123456789; x=$x$x; a=({1..100}); echo ${#x} ${#a[@]} {a,b}{c,d}
### expect
20 100 ac ad bc bd
### end

### bashbox_security_substitutions_fds_pipelines_and_here_documents
# substitutions, fds, pipelines and here-documents
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
exec {fd}>/dev/null; echo $fd $(echo $(echo hi)) | cat | cat; cat <<EOF
end
EOF
### expect
10 hi
end
### end
