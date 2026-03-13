# === ord() ===
# Basic ord operations
assert ord('a') == 97, 'ord lowercase a'
assert ord('A') == 65, 'ord uppercase A'
assert ord('0') == 48, 'ord digit 0'
assert ord(' ') == 32, 'ord space'
assert ord('\n') == 10, 'ord newline'
assert ord('\t') == 9, 'ord tab'

# Unicode characters
assert ord('\u00e9') == 233, 'ord e-acute'
assert ord('\u4e2d') == 20013, 'ord Chinese character'
assert ord('\U0001f600') == 128512, 'ord emoji grinning face'

# === chr() ===
# Basic chr operations
assert chr(97) == 'a', 'chr 97 = a'
assert chr(65) == 'A', 'chr 65 = A'
assert chr(48) == '0', 'chr 48 = 0'
assert chr(32) == ' ', 'chr 32 = space'
assert chr(10) == '\n', 'chr 10 = newline'

# Unicode characters
assert chr(233) == '\u00e9', 'chr 233 = e-acute'
assert chr(20013) == '\u4e2d', 'chr 20013 = Chinese char'
assert chr(128512) == '\U0001f600', 'chr emoji'

# Edge cases
assert chr(0) == '\x00', 'chr 0 = null'
assert chr(0x10FFFF) != '', 'chr max unicode'

# Round-trip test
assert chr(ord('x')) == 'x', 'ord/chr roundtrip'
assert ord(chr(1000)) == 1000, 'chr/ord roundtrip'

# === bin() ===
# Basic bin operations
assert bin(0) == '0b0', 'bin 0'
assert bin(1) == '0b1', 'bin 1'
assert bin(2) == '0b10', 'bin 2'
assert bin(5) == '0b101', 'bin 5'
assert bin(255) == '0b11111111', 'bin 255'
assert bin(-5) == '-0b101', 'bin negative'
assert bin(True) == '0b1', 'bin True'
assert bin(False) == '0b0', 'bin False'
MIN_I64 = -9223372036854775807 - 1  # Smallest i64
MIN_I64_BIN = '1' + '0' * 63
MIN_I64_HEX = '8' + '0' * 15
MIN_I64_OCT = '1' + '0' * 21
assert bin(MIN_I64) == '-0b' + MIN_I64_BIN, 'bin handles i64::MIN without overflow'

# === hex() ===
# Basic hex operations
assert hex(0) == '0x0', 'hex 0'
assert hex(15) == '0xf', 'hex 15'
assert hex(16) == '0x10', 'hex 16'
assert hex(255) == '0xff', 'hex 255'
assert hex(256) == '0x100', 'hex 256'
assert hex(-42) == '-0x2a', 'hex negative'
assert hex(True) == '0x1', 'hex True'
assert hex(False) == '0x0', 'hex False'
assert hex(MIN_I64) == '-0x' + MIN_I64_HEX, 'hex handles i64::MIN without overflow'

# === oct() ===
# Basic oct operations
assert oct(0) == '0o0', 'oct 0'
assert oct(7) == '0o7', 'oct 7'
assert oct(8) == '0o10', 'oct 8'
assert oct(64) == '0o100', 'oct 64'
assert oct(-56) == '-0o70', 'oct negative'
assert oct(True) == '0o1', 'oct True'
assert oct(False) == '0o0', 'oct False'
assert oct(MIN_I64) == '-0o' + MIN_I64_OCT, 'oct handles i64::MIN without overflow'
