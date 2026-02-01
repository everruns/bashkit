### date_year_format
# Get just the year
year=$(date +%Y)
echo "$year" | grep -E '^[0-9]{4}$'
### expect
### end

### date_iso_format
# Get ISO date format
date +%Y-%m-%d | grep -E '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
### expect
### end

### date_time_format
# Get time format
date +%H:%M:%S | grep -E '^[0-9]{2}:[0-9]{2}:[0-9]{2}$'
### expect
### end

### date_epoch
# Get epoch seconds
epoch=$(date +%s)
[ ${#epoch} -ge 10 ] && echo "valid"
### expect
valid
### end
