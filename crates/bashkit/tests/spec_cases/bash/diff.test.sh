### diff_identical
# Identical files produce no output (exit 0)
echo "hello" > /tmp/a.txt
echo "hello" > /tmp/b.txt
diff /tmp/a.txt /tmp/b.txt
echo "exit:$?"
### expect
exit:0
### end

### diff_different
# Different files produce unified diff (exit 1)
echo "hello" > /tmp/a.txt
echo "world" > /tmp/b.txt
diff /tmp/a.txt /tmp/b.txt > /dev/null 2>&1
echo "exit:$?"
### expect
exit:1
### end

### diff_brief
# Brief mode reports file difference
echo "hello" > /tmp/a.txt
echo "world" > /tmp/b.txt
diff -q /tmp/a.txt /tmp/b.txt
### expect
Files /tmp/a.txt and /tmp/b.txt differ
### end

### diff_brief_same
# Brief mode with identical files produces no output
echo "same" > /tmp/a.txt
echo "same" > /tmp/b.txt
diff -q /tmp/a.txt /tmp/b.txt
echo done
### expect
done
### end

### diff_default_normal_format
# diff default format should be normal (ed-style)
echo "a" > /tmp/diff1.txt; echo "b" > /tmp/diff2.txt
diff /tmp/diff1.txt /tmp/diff2.txt | head -1
### expect
1c1
### end

### diff_unified_with_flag
# diff -u should produce unified format (grep for unified marker)
echo "a" > /tmp/diff1.txt; echo "b" > /tmp/diff2.txt
diff -u /tmp/diff1.txt /tmp/diff2.txt | grep -c "^@@"
### expect
1
### end
