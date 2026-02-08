### date_year_format
# Get just the year
year=$(date +%Y)
echo "$year" | grep -qE '^[0-9]{4}$' && echo "valid"
### expect
valid
### end

### date_iso_format
# Get ISO date format
date +%Y-%m-%d | grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' && echo "valid"
### expect
valid
### end

### date_time_format
# Get time format
date +%H:%M:%S | grep -qE '^[0-9]{2}:[0-9]{2}:[0-9]{2}$' && echo "valid"
### expect
valid
### end

### date_epoch
# Get epoch seconds
epoch=$(date +%s)
[ ${#epoch} -ge 10 ] && echo "valid"
### expect
valid
### end

### date_day_format
# Day of month
date +%d | grep -qE '^[0-3][0-9]$' && echo "valid"
### expect
valid
### end

### date_month_format
# Month number
date +%m | grep -qE '^[0-1][0-9]$' && echo "valid"
### expect
valid
### end

### date_hour_format
# Hour (24h)
date +%H | grep -qE '^[0-2][0-9]$' && echo "valid"
### expect
valid
### end

### date_minute_format
# Minute
date +%M | grep -qE '^[0-5][0-9]$' && echo "valid"
### expect
valid
### end

### date_second_format
# Second
date +%S | grep -qE '^[0-6][0-9]$' && echo "valid"
### expect
valid
### end

### date_weekday_short
# Short weekday name
date +%a | grep -qE '^(Mon|Tue|Wed|Thu|Fri|Sat|Sun)$' && echo "valid"
### expect
valid
### end

### date_weekday_full
# Full weekday name
date +%A | grep -qE '^(Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday)$' && echo "valid"
### expect
valid
### end

### date_month_short
# Short month name
date +%b | grep -qE '^(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)$' && echo "valid"
### expect
valid
### end

### date_month_full
# Full month name
date +%B | grep -qE '^(January|February|March|April|May|June|July|August|September|October|November|December)$' && echo "valid"
### expect
valid
### end

### date_12hour
# 12-hour format
date +%I | grep -qE '^(0[1-9]|1[0-2])$' && echo "valid"
### expect
valid
### end

### date_ampm
# AM/PM indicator
date +%p | grep -qE '^(AM|PM)$' && echo "valid"
### expect
valid
### end

### date_day_of_year
# Day of year
date +%j | grep -qE '^[0-3][0-9][0-9]$' && echo "valid"
### expect
valid
### end

### date_week_number
# Week number
date +%U | grep -qE '^[0-5][0-9]$' && echo "valid"
### expect
valid
### end

### date_combined_format
### skip: quoted format string not handling space correctly
# Multiple format specifiers
date +"%Y-%m-%d %H:%M:%S" | grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}$' && echo "valid"
### expect
valid
### end

### date_literal_percent
# Literal percent
date +%% | grep -qE '^%$' && echo "valid"
### expect
valid
### end

### date_rfc_format
### skip: date -R flag not implemented
date -R | grep -qE '^[A-Z][a-z]{2},' && echo "valid"
### expect
valid
### end

### date_iso_flag
### skip: date -I flag not implemented
date -I | grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' && echo "valid"
### expect
valid
### end

### date_utc_flag
date -u +%Z | grep -qE '^UTC$' && echo "valid"
### expect
valid
### end

### date_date_string
date -d '2024-01-15T12:00:00' +%Y-%m-%d
### expect
2024-01-15
### end

### date_relative_yesterday
date -d 'yesterday' +%Y-%m-%d | grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' && echo "valid"
### expect
valid
### end

### date_relative_tomorrow
date -d 'tomorrow' +%Y-%m-%d | grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' && echo "valid"
### expect
valid
### end

### date_set_time
### skip: date -s (set time) not implemented and requires privileges
date -s '2024-01-01 12:00:00' 2>/dev/null || echo "skip"
### expect
skip
### end

### date_timezone
### skip: timezone abbreviation format varies
# Timezone abbreviation
date +%Z | grep -qE '^[A-Z]{3,4}$' && echo "valid"
### expect
valid
### end

### date_nanoseconds
### skip: %N (nanoseconds) not implemented
date +%N | grep -qE '^[0-9]{9}$' && echo "valid"
### expect
valid
### end

### date_century
# Century
date +%C | grep -qE '^[0-9]{2}$' && echo "valid"
### expect
valid
### end

### date_day_space_padded
# Day space-padded
date +%e | grep -qE '^[ 1-3][0-9]$' && echo "valid"
### expect
valid
### end

### date_weekday_number
# Day of week (0-6, Sunday=0)
date +%w | grep -qE '^[0-6]$' && echo "valid"
### expect
valid
### end

### date_relative_days_ago
date -d '30 days ago' +%Y-%m-%d | grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' && echo "valid"
### expect
valid
### end

### date_relative_epoch
date -d '@0' +%Y-%m-%d
### expect
1970-01-01
### end

### date_compound_date_minus_days
date -d '2024-06-15 - 30 days' +%Y-%m-%d
### expect
2024-05-16
### end

### date_compound_date_plus_days
date -d '2024-01-15 + 30 days' +%Y-%m-%d
### expect
2024-02-14
### end

### date_compound_epoch_minus_day
### bash_diff: GNU date doesn't support @epoch with compound modifiers
date -d '@1700000000 - 1 day' +%s
### expect
1699913600
### end

### date_compound_yesterday_plus_hours
date -d 'yesterday + 12 hours' +%Y-%m-%d | grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' && echo "valid"
### expect
valid
### end
