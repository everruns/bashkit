### awk_delete_entire_array
# delete array clears all elements
echo "test" | awk 'BEGIN{a[1]=1; a[2]=2; a[3]=3; delete a; print length(a)}'
### expect
0
### end

### awk_delete_single_element
# delete array[key] removes one element
echo "test" | awk 'BEGIN{a[1]=1; a[2]=2; a[3]=3; delete a[2]; for(k in a) print k, a[k]}' | sort
### expect
1 1
3 3
### end

### awk_delete_multiple_arrays
# delete works on multiple arrays
echo "test" | awk 'BEGIN{a[1]=1; b[1]=2; delete a; delete b; print length(a), length(b)}'
### expect
0 0
### end
