import json,random,subprocess
from pathlib import Path

cases=[]
def add(script,**options): cases.append({'script':script,**options})
for n in (0,1,2,7,30):
 add(f'total=0; for ((i=0;i<{n};i++)); do ((total+=i)); done; printf "%s:%s\\n" "$total" "$i"')
 add(f'a=0; while ((a<{n})); do ((a++)); done; printf "%s:%s\\n" "$a" "$?"')
 add(f'a=0; until ((a=={n})); do ((a++)); done; printf "%s:%s\\n" "$a" "$?"')
ordinary=[
 'a=2; ((a+=3)); if ((a<10)); then ((a*=2)); else a=0; fi; echo "$a:$?"',
 'a=0; if ((a)); then a=2; elif ((1)); then a=3; else a=4; fi; echo "$a:$?"',
 'if ((0)); then a=1; fi; echo "$?:${a-unset}"',
 'a=0; { ((a++)); ((a+=7)); }; echo "$a:$?"',
 'total=0; for ((i=0;i<5;i++)); do for ((j=0;j<3;j++)); do ((total+=i+j)); done; done; echo "$total:$i:$j:$?"',
 'limit=6; for ((i=0;i<limit;i++)); do ((limit--)); done; echo "$i:$limit:$?"',
 'a=0; for ((i=0,j=2;i<5;i++,j+=2)); do ((a+=j)); done; echo "$a:$i:$j:$?"',
 'i=0; for ((;i<3;)); do ((i++)); done; echo "$i:$?"',
 'for ((i=0;i<4;i++)); do ((0)); done; echo "$?:$i:${PIPESTATUS[*]}"',
 'while ((0)); do ((1)); done; echo "$?:${PIPESTATUS[*]}"',
 'a=0; while ((a++<3)); do ((a+=1)); done; echo "$a:$?"',
 'a=0; ((a=1, a++, a*4)); echo "$a:$?"',
 'a=0; ((a++)) || ((a+=3)); echo "$a:$?"',
 'a=1; ((a++)) && ((a+=3)); echo "$a:$?"',
 'x=9223372036854775807; ((x++)); echo "$x:$?"',
 'x=0x10; ((x+=1)); echo "$x:$?"',
 'x=010; ((x+=1)); echo "$x:$?"',
 'x=-5; ((x=x*2-1)); echo "$x:$?"',
 '((missing+=3)); echo "$missing:$?"',
 'a=0; for ((i=0;i<4;i++)); do if ((i&1)); then ((a+=2)); else ((a+=i)); fi; done; echo "$a:$?"',
 'a=1; ((a<<=2)); echo "$a:$?"',
 'a=0; for ((i=0;i<6;i+=2)); do ((a+=i)); done; echo "$a:$?"',
 'a=0; for ((i=0;i<4;i++)); do printf "%s\\n" "$i"; ((a+=i)); done; echo "$a:$?"',
 'for ((i=0;i<6;i++)); do if ((i==2)); then continue; fi; if ((i==4)); then break; fi; echo "$i"; done; echo "$i:$?"',
 'a=0; for ((i=0;i<$(printf 3);i++)); do ((a+=i)); done; echo "$a:$?"',
 'x="2+3"; ((x+=2)); echo "$x:$?"',
 'x=08; ((x+=2)); echo "$x:$?"',
 'declare -i x=2; ((x+=2)); echo "$x:$?"',
 'readonly x=2; ((x+=2)); echo "$x:$?"',
 'declare -n x=actual; actual=2; ((x+=2)); echo "$x:$actual:$?"',
 'a=(1 2); ((a[0]++)); echo "${a[*]}:$?"',
 'a=(1 2); ((a++)); echo "${a[*]}:$?"',
 'a=1; ((a+=3)); echo "$((a*2)):$a:$?"',
 'a=1; ((a && a++)); echo "$a:$?"',
 'a=0; ((a ? 3 : a++)); echo "$a:$?"',
 '((1/0)); echo after:$?',
 '((2**5)); echo after:$?',
 'set -e; a=0; for ((i=0;i<4;i++)); do ((a+=i)); done; echo after',
 'set -u; ((missing+=1)); echo after',
 'set -x; a=0; for ((i=0;i<2;i++)); do ((a++)); done; echo "$a"',
 'set -n; a=0; for ((i=0;i<2;i++)); do ((a++)); done; echo "$a"',
 "trap 'echo debug' DEBUG; a=0; for ((i=0;i<2;i++)); do ((a++)); done; echo $a",
 "trap 'echo err' ERR; ((0)); echo after",
 "trap 'echo exit:$a' EXIT; a=0; for ((i=0;i<3;i++)); do ((a++)); done",
 "a=0; trap 'echo debug' DEBUG; ((a++)); echo $a",
 "a=0; eval 'for ((i=0;i<3;i++)); do ((a++)); done'; echo $a:$i",
 "a=0; f() { local a=10; for ((i=0;i<3;i++)); do ((a++)); done; echo $a; }; f; echo $a:$i",
 "printf 'for ((i=0;i<3;i++)); do ((a++)); done\\n' > /code; a=0; source /code; echo $a:$i",
 "shopt -s expand_aliases\nalias hi='echo ok'\na=0\n((a++))\nhi\necho $a",
 'a=0; ((a++)); exit 7; ((a++))',
 'a=0; ((a++)) > /out; echo "$a:$?"; cat /out',
 'exec > /out; a=0; ((a++)); printf "%s\\n" "$a"; cat /out >&2',
 'a=0; ((a++)); printf "%s\\n" "$a" | cat; v=$(printf hello); echo "$v:$a"',
 'a=0\nfor ((i=0;i<3;i+=LINENO)); do\n ((a++))\ndone\nprintf "%s:%s:%s\\n" "$a" "$i" "$LINENO"',
 'a=0\nwhile ((a<3)); do\n ((a++))\ndone\nprintf "%s:%s\\n" "$a" "$LINENO"',
 'a=0; ( ((a++)); echo "$a" ); echo "$a"',
 'a=0; ((a++)) & wait; echo "$a"',
 'label=hello\na=0\nfor ((i=0;i<4;i++)); do ((a+=i)); done\necho "$label:$a"',
]
for script in ordinary:add(script)
rng=random.Random(7413)
for _ in range(100):
 n=rng.randrange(0,25); a=rng.randrange(-30,30); b=rng.randrange(-10,11)
 op=rng.choice(['+=','-=','*='])
 add(f'a={a}; for ((i=0;i<{n};i++)); do if ((i&1)); then ((a{op}{b})); else ((a+=i)); fi; done; printf "%s:%s:%s\\n" "$a" "$i" "$?"')
for field in ('commands','iterations','work'):
 for ceiling in (0,1,5,100):add('a=0; for ((i=0;i<100;i++)); do ((a+=i)); done',**{field:ceiling})
for ceiling in (0,128,256,512,1024,4096,16384,65536,1048576):add('for ((i=0;i<3;i++)); do ((a+=i)); done; echo "$a"',live=ceiling)
add('for ((;;)); do ((a++)); done',cancelled=True)
add('for ((;;)); do ((a++)); done',timeout_ms=1,iterations=100000000)
cases += [ {'scripts':['a=0; for ((i=0;i<4;i++)); do ((a+=i)); done','echo "$a:$i"; ((a+=7)); echo "$a"']},
           {'scripts':['readonly a=2','((a++)); echo "$a:$?"']},
           {'scripts':['a="2+3"','((a++)); echo "$a:$?"']}]
outputs={}
payload=''.join(json.dumps(c)+'\n' for c in cases)
for build in ('baseline','candidate'):
 result=subprocess.run([f'./{build}','run'],input=payload,text=True,capture_output=True,check=True)
 outputs[build]=[json.loads(line) for line in result.stdout.splitlines()]
 assert len(outputs[build])==len(cases)
failures=[]
for case,before,after in zip(cases,outputs['baseline'],outputs['candidate']):
 if before==after:continue
 # Accounting includes admitted compilation work/storage. Narrow resource
 # probes compare rejection outcomes, allowing diagnostic quantities to differ.
 resources=any(k in case for k in ('live','commands','iterations','work','timeout_ms'))
 if resources and all('error' in row for result in (before,after) for row in result):continue
 failures.append({'case':case,'before':before,'after':after})
Path('differential-results.json').write_text(json.dumps({'cases':cases,'outputs':outputs,'failures':failures},indent=2)+'\n')
print(f'{len(cases)-len(failures)}/{len(cases)} differential cases passed')
for failure in failures: print(json.dumps(failure))
raise SystemExit(bool(failures))
