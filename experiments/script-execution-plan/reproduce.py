"""Build matched binaries from two checkouts, then compare correctness/time."""
import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path

parser=argparse.ArgumentParser(description=__doc__)
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
metadata={}
for build,repo in [('baseline',args.baseline_repo),('candidate',args.candidate_repo)]:
 repo=repo.resolve()
 project=out/(build+'-pilot');(project/'src').mkdir(parents=True)
 for name in ('Cargo.lock','rust-toolchain.toml'):shutil.copy2(source/name,project/name)
 shutil.copy2(source/'src/main.rs',project/'src/main.rs')
 manifest=(source/'Cargo.toml').read_text().replace('"../../crates/bashkit"',json.dumps(str(repo/'crates/bashkit')))
 if build=='baseline':manifest=manifest.replace('[features]\nplanned = ["bashkit/experimental_execution_plan"]\n','')
 (project/'Cargo.toml').write_text(manifest)
 command=['cargo','build','--locked','--release']
 if build=='candidate':command+=['--features','planned']
 with (out/(build+'-build.log')).open('w') as log:
  subprocess.run(command,cwd=project,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
 binary='bashkit-script-plan-pilot'+('.exe' if os.name=='nt' else '')
 shutil.copy2(out/'target/release'/binary,out/(build+('.exe' if os.name=='nt' else '')))
 metadata[build]=subprocess.check_output(['git','-C',str(repo),'rev-parse','HEAD'],text=True).strip()
 print(f'{build} built',flush=True)
metadata['benchmark']=subprocess.check_output(['git','-C',str(args.benchmark_repo.resolve()),'rev-parse','HEAD'],text=True).strip()
(out/'builds.json').write_text(json.dumps(metadata,indent=2)+'\n')
for script in ('differential.py','compare.py'):
 subprocess.run([os.sys.executable,str(out/script)],cwd=out,check=True)
print(f'Results: {out}')
