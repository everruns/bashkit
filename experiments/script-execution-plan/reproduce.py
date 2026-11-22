"""Build matched binaries from two checkouts, then compare correctness/time."""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--allocation-profile',action='store_true',help='Also collect allocation counts (separate instrumented binaries)')
parser.add_argument('--same-binary',action='store_true',help='Also compare AST and automatic admission inside one binary')
parser.add_argument('--baseline-repo',type=Path,required=True)
parser.add_argument('--candidate-repo',type=Path,required=True)
parser.add_argument('--benchmark-repo',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True,help='New or empty directory for projects, binaries and results')
args=parser.parse_args()
source=Path(__file__).resolve().parent
out=args.output.resolve()
if out.exists() and (not out.is_dir() or any(out.iterdir())):parser.error('--output must be empty')
out.mkdir(parents=True,exist_ok=True)
(out/'code-mode-language-benchmark').symlink_to(args.benchmark_repo.resolve(),target_is_directory=True)
for name in ('differential.py','compare.py'):shutil.copy2(source/name,out/name)
env=os.environ.copy()
env.update(RUSTUP_TOOLCHAIN='1.100.0',CARGO_TARGET_DIR=str(out/'target'),CARGO_INCREMENTAL='0',CARGO_BUILD_JOBS='2')
metadata={'sha256':{}}
for build,repo in [('baseline',args.baseline_repo),('candidate',args.candidate_repo)]:
 repo=repo.resolve()
 project=out/(build+'-pilot');(project/'src').mkdir(parents=True)
 for name in ('Cargo.lock','rust-toolchain.toml'):shutil.copy2(source/name,project/name)
 shutil.copy2(source/'src/main.rs',project/'src/main.rs')
 manifest=(source/'Cargo.toml').read_text().replace('"../../crates/bashkit"',json.dumps(str(repo/'crates/bashkit')))
 if build=='baseline':
  manifest=manifest.replace('planned = ["bashkit/experimental_execution_plan"]','planned = []').replace('same_binary = ["bashkit/experimental_plan_benchmark_control"]','same_binary = []')
 (project/'Cargo.toml').write_text(manifest)
 command=['cargo','build','--locked','--release']
 if build=='candidate':command+=['--features','planned']
 with (out/(build+'-build.log')).open('w') as log:
  subprocess.run(command,cwd=project,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
 binary='bashkit-script-plan-pilot'+('.exe' if os.name=='nt' else '')
 shutil.copy2(out/'target/release'/binary,out/(build+('.exe' if os.name=='nt' else '')))
 metadata[build]=subprocess.check_output(['git','-C',str(repo),'rev-parse','HEAD'],text=True).strip()
 metadata['sha256'][build]=hashlib.sha256((out/build).read_bytes()).hexdigest()
 metadata['sha256']['Cargo.lock']=hashlib.sha256((project/'Cargo.lock').read_bytes()).hexdigest()
 print(f'{build} built',flush=True)
metadata['benchmark']=subprocess.check_output(['git','-C',str(args.benchmark_repo.resolve()),'rev-parse','HEAD'],text=True).strip()
(out/'builds.json').write_text(json.dumps(metadata,indent=2)+'\n')
for script in ('differential.py','compare.py'):
 subprocess.run([os.sys.executable,str(out/script)],cwd=out,check=True)

if args.same_binary:
 project=out/'candidate-pilot'
 with (out/'same-binary-build.log').open('w') as log:
  subprocess.run(['cargo','build','--locked','--release','--features','same_binary'],cwd=project,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
 shutil.copy2(out/'target/release'/binary,out/'same-binary')
 metadata['sha256']['same-binary']=hashlib.sha256((out/'same-binary').read_bytes()).hexdigest()
 shutil.copy2(source/'same_binary.py',out/'same_binary.py')
 subprocess.run([os.sys.executable,str(out/'same_binary.py')],cwd=out,check=True)
if args.allocation_profile:
 rows=[]
 for build in ('baseline','candidate'):
  features='allocation_profile'+(',planned' if build=='candidate' else '')
  with (out/(build+'-profile-build.log')).open('w') as log:
   subprocess.run(['cargo','build','--locked','--release','--features',features],cwd=out/(build+'-pilot'),env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  shutil.copy2(out/'target/release'/binary,out/(build+'-profile'))
  metadata['sha256'][build+'-profile']=hashlib.sha256((out/(build+'-profile')).read_bytes()).hexdigest()
  run=subprocess.run([str(out/(build+'-profile'))],capture_output=True,text=True,check=True)
  rows.extend({'build':build,**json.loads(line)} for line in run.stdout.splitlines())
 (out/'allocations.json').write_text(json.dumps(rows,indent=2)+'\n')

(out/'builds.json').write_text(json.dumps(metadata,indent=2)+'\n')
print(f'Results: {out}')
