"""Alternate interpreter and automatic admission in one diagnostic binary."""
import json
import os
import statistics
import subprocess
from pathlib import Path

cpu=sorted(os.sched_getaffinity(0))[3]
runs=[]
for round_number in range(5):
    result=subprocess.run(['taskset','-c',str(cpu),'./same-binary','--same-binary'],capture_output=True,text=True,check=True)
    for line in result.stdout.splitlines():
        data=json.loads(line)
        assert len(data['ast_us'])==len(data['auto_us'])==200
        runs.append({'round':round_number,**data})
summary={}
for name in dict.fromkeys(r['workload'] for r in runs):
    rows=[r for r in runs if r['workload']==name]
    summary[name]={key:statistics.median(statistics.median(r[key]) for r in rows) for key in ('ast_us','auto_us')}
    summary[name]['per_round_change_percent']=[100*(statistics.median(r['auto_us'])/statistics.median(r['ast_us'])-1) for r in rows]
    print(name,summary[name],flush=True)
Path('same-binary.json').write_text(json.dumps({'cpu':cpu,'summary':summary,'runs':runs},indent=2)+'\n')
