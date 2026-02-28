### file_text
# file detects ASCII text
echo "hello world" > /tmp/file_test.txt
file /tmp/file_test.txt
### expect
/tmp/file_test.txt: ASCII text
### end

### file_empty
# file detects empty file
touch /tmp/empty_file
file /tmp/empty_file
### expect
/tmp/empty_file: empty
### end

### file_directory
# file detects directory
mkdir -p /tmp/filedir
file /tmp/filedir
### expect
/tmp/filedir: directory
### end

### file_script_bash
### bash_diff: VFS file outputs different format than real file
# file detects bash scripts
printf '#!/bin/bash\necho hi\n' > /tmp/script.sh
file /tmp/script.sh
### expect
/tmp/script.sh: Bourne-Again shell script
### end

### file_script_python
### bash_diff: VFS file outputs different format than real file
# file detects python scripts
printf '#!/usr/bin/env python3\nprint("hi")\n' > /tmp/script.py
file /tmp/script.py
### expect
/tmp/script.py: Python script
### end

### file_json
### bash_diff: VFS file outputs different format than real file
# file detects JSON
echo '{"key":"value"}' > /tmp/data.json
file /tmp/data.json
### expect
/tmp/data.json: JSON text
### end

### file_nonexistent
### bash_diff: bashkit file returns 0 with error in stdout
# file on nonexistent path
file /tmp/nonexistent_xyz_file 2>&1 | grep -q "cannot open" && echo "error shown"
### expect
error shown
### end

### file_multiple
### bash_diff: VFS file outputs different format than real file
# file handles multiple files
echo "text" > /tmp/multi1.txt
mkdir -p /tmp/multi2
file /tmp/multi1.txt /tmp/multi2
### expect
/tmp/multi1.txt: ASCII text
/tmp/multi2: directory
### end
