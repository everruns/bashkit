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
