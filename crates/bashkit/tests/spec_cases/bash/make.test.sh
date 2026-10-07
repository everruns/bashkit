# make: GNU make subset. Cases are checked against GNU make 4.3.

### make_basic_build_and_up_to_date
d=$(mktemp -d); cd "$d"
printf 'out.txt: in.txt\n\tcp in.txt out.txt\n' > Makefile
echo hi > in.txt
make; make; cat out.txt
### expect
cp in.txt out.txt
make: 'out.txt' is up to date.
hi
### end

### make_rebuilds_when_prereq_changes
d=$(mktemp -d); cd "$d"
printf 'out: in\n\t@cat in > out; echo built\n' > Makefile
echo 1 > in; make
sleep 1; echo 2 > in; make; cat out
### expect
built
built
2
### end

### make_phony_default_goal_and_nothing_to_do
d=$(mktemp -d); cd "$d"
printf '.PHONY: all clean\nall: a\na:\n\ttouch a\nclean:\n\trm -f a\n' > Makefile
make; make; make clean; make all
### expect
touch a
make: Nothing to be done for 'all'.
rm -f a
touch a
### end

### make_automatic_variables
d=$(mktemp -d); cd "$d"
touch x.c y.c
printf 'prog: x.o y.o\n\t@echo "link $@ from $^ first $<"\n%%.o: %%.c\n\t@echo "compile $< -> $@ stem $*"\n\t@touch $@\n' > Makefile
make
### expect
compile x.c -> x.o stem x
compile y.c -> y.o stem y
link prog from x.o y.o first x.o
### end

### make_variables_and_functions
d=$(mktemp -d); cd "$d"
cat > Makefile <<'MK'
SRC = main.c util.c lib/io.c
OBJ := $(SRC:.c=.o)
NAMES = $(notdir $(basename $(SRC)))
CFLAGS ?= -O2
CFLAGS += -Wall
upper = $(subst a,A,$(1))
all:
	@echo "$(OBJ)"
	@echo "$(NAMES) $(words $(SRC)) $(word 2,$(SRC))"
	@echo "$(CFLAGS) $(call upper,banana)"
	@echo "$(patsubst %.c,%.h,$(SRC)) $(filter lib/%,$(SRC))"
	@echo "$(foreach n,$(NAMES),[$(n)]) $(sort b a c a)"
	@echo "$(if $(SRC),yes,no) $(origin CFLAGS) $(dir lib/io.c)"
MK
make
make CFLAGS=-g
### expect
main.o util.o lib/io.o
main util io 3 util.c
-O2 -Wall bAnAnA
main.h util.h lib/io.h lib/io.c
[main] [util] [io] a b c
yes file lib/
main.o util.o lib/io.o
main util io 3 util.c
-g bAnAnA
main.h util.h lib/io.h lib/io.c
[main] [util] [io] a b c
yes command line lib/
### end

### make_conditionals_and_define
d=$(mktemp -d); cd "$d"
cat > Makefile <<'MK'
MODE ?= debug
ifeq ($(MODE),release)
FLAGS = -O2
else ifeq ($(MODE),debug)
FLAGS = -g
else
FLAGS = none
endif
ifdef UNSET
X = set
endif
define banner
@echo ===
@echo $(1)
endef
all:
	$(call banner,$(FLAGS)$(X))
MK
make; make MODE=release; make MODE=other
### expect
===
-g
===
-O2
===
none
### end

### make_recipe_prefixes_and_errors
d=$(mktemp -d); cd "$d"
printf 'a:\n\t-false\n\t@echo after\nb:\n\tfalse\n\techo never\n' > Makefile
make a 2>e; echo rc=$?; cat e
make b 2>e; echo rc=$?; cat e
make -i b 2>e; echo rc=$?; cat e
### expect
false
after
rc=0
make: [Makefile:2: a] Error 1 (ignored)
false
rc=2
make: *** [Makefile:5: b] Error 1
false
echo never
never
rc=0
make: [Makefile:5: b] Error 1 (ignored)
### end

### make_missing_rule_and_makefile
d=$(mktemp -d); cd "$d"
make 2>&1; echo rc=$?
touch exists; make exists; echo rc=$?
make nope 2>&1; echo rc=$?
printf 'a: b\n\t@echo a\n' > Makefile
make 2>&1; echo rc=$?
### expect
make: *** No targets specified and no makefile found.  Stop.
rc=2
make: Nothing to be done for 'exists'.
rc=0
make: *** No rule to make target 'nope'.  Stop.
rc=2
make: *** No rule to make target 'b', needed by 'a'.  Stop.
rc=2
### end

### make_missing_separator
d=$(mktemp -d); cd "$d"
printf 'x:\n    echo hi\n' > Makefile
make 2>&1; echo rc=$?
### expect
Makefile:2: *** missing separator.  Stop.
rc=2
### end

### make_dry_run_and_silent
d=$(mktemp -d); cd "$d"
printf 'all:\n\t@echo one\n\techo two\n' > Makefile
make -n; make -s
### expect
echo one
echo two
one
two
### end

### make_question_and_always
d=$(mktemp -d); cd "$d"
printf 'f: g\n\t@cp g f; echo copied\n' > Makefile
touch g
make -q; echo q=$?
make; make -q; echo q=$?
make -B
### expect
q=1
copied
q=0
copied
### end

### make_file_and_directory_options
d=$(mktemp -d); cd "$d"
mkdir sub
printf 'hello:\n\t@echo in $(notdir $(CURDIR))\n' > sub/build.mk
make -s -C sub -f build.mk
make --no-print-directory --directory=sub --file=build.mk hello
### expect
in sub
in sub
### end

### make_recursive_make_passes_variables
d=$(mktemp -d); cd "$d"
mkdir lib
printf 'all:\n\t@echo lib $(V) $(MAKELEVEL)\n' > lib/Makefile
printf 'all:\n\t@$(MAKE) -s -C lib\n' > Makefile
make -s V=7
### expect
lib 7 1
### end

### make_shell_wildcard_include
d=$(mktemp -d); cd "$d"
touch b.c a.c skip.h
printf 'EXTRA = from-include\n' > vars.mk
cat > Makefile <<'MK'
include vars.mk
-include missing.mk
SRC := $(wildcard *.c)
COUNT != echo $(SRC) | wc -w
all:
	@echo $(SRC) $(COUNT) $(EXTRA) $(shell echo sh)
MK
make
### expect
a.c b.c 2 from-include sh
### end

### make_static_pattern_order_only_target_vars
d=$(mktemp -d); cd "$d"
touch a.in b.in
cat > Makefile <<'MK'
OUTS = a.out b.out
all: $(OUTS)
$(OUTS): %.out: %.in | outdir
	@echo "$< -> $@ [$(TAG)]"
outdir:
	@echo mkdir
b.out: TAG = special
.PHONY: all outdir
MK
make
### expect
mkdir
a.in -> a.out []
b.in -> b.out [special]
### end

### make_exported_variables_reach_recipes
d=$(mktemp -d); cd "$d"
printf 'export GREETING = hello\nPLAIN = no\nall:\n\t@echo "[$$GREETING][$$PLAIN]"\n' > Makefile
make
### expect
[hello][]
### end

### make_keep_going
d=$(mktemp -d); cd "$d"
printf 'all: bad good\nbad:\n\t@false\ngood:\n\t@echo good\n' > Makefile
make -k 2>e; echo rc=$?; cat e
### expect
good
rc=2
make: *** [Makefile:3: bad] Error 1
make: Target 'all' not remade because of errors.
### end

### make_suffix_rule
d=$(mktemp -d); cd "$d"
touch m.c
printf '.c.o:\n\t@echo suffix $< $@\n' > Makefile
make m.o
### expect
suffix m.c m.o
### end

### make_error_and_info_functions
d=$(mktemp -d); cd "$d"
printf '$(info parsing)\nifndef NEED\n$(error NEED is not set)\nendif\nall: ; @echo ok\n' > Makefile
make 2>e; echo rc=$?; cat e
make NEED=1
### expect
parsing
rc=2
Makefile:3: *** NEED is not set.  Stop.
parsing
ok
### end
