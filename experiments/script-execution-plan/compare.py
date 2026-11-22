import json,os,statistics,subprocess
from pathlib import Path

cpu=sorted(os.sched_getaffinity(0))[3]
workloads=['short_tool','short_arithmetic','straight_arithmetic','while_1000','branch_loop','assignment_loop','pipeline_fallback','substitution_fallback','mixed_script','arithmetic_for_100','arithmetic_for_1000','arithmetic_for_5000','existing_fanout']
runs=[]
for workload in workloads:
 for round_number in range(5):
  order=('baseline','candidate') if round_number%2==0 else ('candidate','baseline')
  for build in order:
   run=subprocess.run(['taskset','-c',str(cpu),f'./{build}',workload],text=True,capture_output=True,check=True)
   data=json.loads(run.stdout)
   assert len(data['samples_us'])==len(data['cold_samples_us'])==60
   runs.append({'build':build,'round':round_number,**data})
 print(workload,flush=True)
summary={}
for workload in workloads:
 summary[workload]={}
 for build in ('baseline','candidate'):
  selected=[r for r in runs if r['workload']==workload and r['build']==build]
  summary[workload][build]={key:statistics.median(statistics.median(r[key]) for r in selected) for key in ('samples_us','cold_samples_us')}
Path('comparison.json').write_text(json.dumps({'cpu':cpu,'summary':summary,'runs':runs},indent=2)+'\n')
for name,values in summary.items():
 b=values['baseline']['samples_us'];a=values['candidate']['samples_us']
 print(f'{name}: {b:.3f} -> {a:.3f} us ({100*(1-a/b):+.1f}% reduction)',flush=True)
