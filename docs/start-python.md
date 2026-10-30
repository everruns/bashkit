# Get started in Python

Embed the Bashkit sandbox in a Python application or agent. The package ships as
pre-built binary wheels on PyPI, install and go, no Rust toolchain needed.

## Install

```bash
pip install bashkit
```

## First script

```python
from bashkit import Bash

bash = Bash()
result = bash.execute_sync("echo 'Hello, World!'")
print(result.stdout)
```

## Persistent state

A `Bash` instance keeps its environment and virtual filesystem across calls:

```python
from bashkit import Bash

bash = Bash()
bash.execute_sync("export APP_ENV=dev")
print(bash.execute_sync("echo $APP_ENV").stdout)  # dev
```

## Sync vs async

`execute_sync()` runs scripts that complete without suspending, plain bash and
`jq`. If you register an async custom builtin (e.g. one that issues an HTTP
request), use the awaitable `execute()` instead:

```python
result = await bash.execute("echo hi | my_async_tool")
```

## Embedded Python and TypeScript

Native wheels can run Python and TypeScript *inside* the shell. Both runtimes
are disabled by default; opt in with `python=True` and `typescript=True`.
Python also requires a host-supplied environment gate:

```python
from bashkit import BashTool

bash = BashTool(
    python=True,
    typescript=True,
    env={"BASHKIT_ALLOW_INPROCESS_PYTHON": "1"},
)
assert bash.execute_sync("python -c 'print(42)'").stdout == "42\n"
assert bash.execute_sync("ts -c '42'").stdout == "42\n"
```

Scripts cannot grant Python permission by exporting the gate variable. Runtime
flags survive `reset()`; supply them again when constructing from a snapshot.
LangChain's `BashkitTool` and deepagents' middleware/backend accept the same
flags.

This differs from embedding Bashkit in your Python application; see the
[Python builtin](../crates/bashkit/docs/python.md) and
[TypeScript builtin](../crates/bashkit/docs/typescript.md) guides.

## Examples

Runnable Python examples in the repo:

- [`bash_basics.py`](https://github.com/everruns/bashkit/blob/main/crates/bashkit-python/examples/bash_basics.py), first scripts and persistent state
- [`data_pipeline.py`](https://github.com/everruns/bashkit/blob/main/crates/bashkit-python/examples/data_pipeline.py), pipes and data processing
- [`llm_tool.py`](https://github.com/everruns/bashkit/blob/main/crates/bashkit-python/examples/llm_tool.py), exposing Bashkit as an LLM tool
- Agent integrations: [deepagents](https://github.com/everruns/bashkit/blob/main/examples/deepagent_coding_agent.py), [Pydantic AI](https://github.com/everruns/bashkit/blob/main/examples/pydantic_ai_bash_agent.py)

## Windows 11 Smart App Control

If importing Bashkit raises `DLL load failed ... An Application Control policy
has blocked this file`, first identify the file in Windows' Code Integrity log.
The same error can come from a dependency or another App Control policy.
[PondGuard reported](https://github.com/onlyarya2004-maker/pondguard/commit/2f7c1a7770930b6bc1f7a4928376816668250c00)
that Bashkit 0.20.1 was blocked on its Windows 11 / Python 3.11.9 laptop, while
0.20.0 loaded. This is a machine-specific report, not a guarantee that 0.20.0
will be allowed on every machine.

### Immediate workaround

Keep Smart App Control enabled. In the affected application's virtual environment:

```powershell
python -m pip install --only-binary=:all: --no-cache-dir "bashkit==0.20.0"
python -m pip check
python -c "from bashkit import BashTool; r = BashTool().execute_sync('echo hello'); assert r.exit_code == 0; print(r.stdout)"
python -m pytest
```

Persist the pin when resolving the application's dependencies. For example, add
`bashkit==0.20.0 ; sys_platform == 'win32'` to a constraints file and use
`python -m pip install -r requirements.txt -c constraints-windows.txt` on every
install. An existing lockfile must also retain the pin. Recheck the pin after a
signed release has been tested on the affected machine.

`python -m pytest` avoids a blocked `pytest.exe` launcher; it cannot bypass a
blocked `.pyd`. If only unrelated pytest plugin discovery imports Bashkit,
`$env:PYTEST_DISABLE_PLUGIN_AUTOLOAD = "1"` can let independent tests run, but
explicitly enable each required plugin with `-p`. This disables plugin-provided
fixtures and hooks and does not repair Bashkit. Remove the variable afterward
with `Remove-Item Env:PYTEST_DISABLE_PLUGIN_AUTOLOAD`.

### Reproduce and collect evidence

Use a Windows 11 x64 machine with Smart App Control **On**, confirmed in Windows
Security → App & browser control → Smart App Control settings. Evaluation mode
does not prove enforcement. Use Python 3.11.9 x64 to match the report, record
`py -3.11 --version`, and leave policy settings unchanged. Normal hosted Windows
CI and Linux import tests do not establish SAC compatibility.

Run this PowerShell comparison from a writable folder. Each version gets a
fresh environment and the published binary wheel; no source build or pip cache
is used. Signatures and hashes are collected without importing Bashkit:

```powershell
Start-Transcript -Path ("sac-" + (Get-Date -Format "yyyyMMdd-HHmmss") + ".txt")
$started = Get-Date
Get-ComputerInfo | Select-Object WindowsProductName, OsVersion, OsBuildNumber
py -3.11 --version
py -3.11 -c "import platform, struct; print(platform.machine(), struct.calcsize('P') * 8)"
foreach ($version in @("0.20.0", "0.20.1")) {
    $venv = ".sac-$version"
    if (Test-Path $venv) { throw "$venv already exists; use a fresh folder" }
    py -3.11 -m venv $venv
    if ($LASTEXITCODE -ne 0) { throw "venv creation failed" }
    $python = "$venv\Scripts\python.exe"
    & $python -m pip install --index-url https://pypi.org/simple --only-binary=:all: --no-cache-dir --report "$version-install.json" "bashkit==$version"
    if ($LASTEXITCODE -ne 0) { throw "installation failed for $version" }
    Get-ChildItem "$venv\Lib\site-packages\bashkit" -Recurse -Include *.pyd,*.dll |
        ForEach-Object {
            Get-FileHash $_.FullName -Algorithm SHA256
            Get-AuthenticodeSignature $_.FullName |
                Format-List Path, Status, StatusMessage, SignerCertificate, TimeStamperCertificate
        }
    & $python -c "from bashkit import BashTool; r = BashTool().execute_sync('echo hello'); assert r.exit_code == 0; print(r.stdout)" 2>&1 |
        Tee-Object "$version-import.txt"
    "Import exit code for ${version}: $LASTEXITCODE"
}
Get-WinEvent -FilterHashtable @{
    LogName = "Microsoft-Windows-CodeIntegrity/Operational"
    StartTime = $started
} | ForEach-Object { $_.ToXml() } | Set-Content sac-codeintegrity.xml
Stop-Transcript
```

Save the terminal output too. If log access is denied, collect it from an
elevated PowerShell window. A query with no matching events is not proof of a
SAC block. Inspect event **3077** for the enforced block, its file path and
policy identity; correlate **3089** signature events by ActivityID. **3076** is
an audit event. Check whether the blocked path is `_bashkit.pyd`, another DLL,
or a launcher, and whether the policy is SAC or a separately managed policy.
See Microsoft's [Code Integrity event reference](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/app-control-for-business/operations/event-id-explanations).

For the pytest symptom, use the affected project's environment and keep every
other dependency fixed. Record `python -m pip freeze` and `python -m pytest
--trace-config --collect-only` with each Bashkit version; identify the importing
plugin from the traceback. Swap only Bashkit with `python -m pip install
--only-binary=:all: --no-cache-dir --no-deps --force-reinstall "bashkit==0.20.1"`,
then repeat with `0.20.0`. Reapply the working pin afterward. Installing only
Bashkit and pytest in a clean environment does not recreate the project's
transitive plugin imports.

### Artifact comparison and permanent fix

The published `cp39-abi3-win_amd64` wheels were inspected on 2026-10-27. Their
SHA-256 digests matched PyPI's release JSON. Both use maturin 1.15.0 and contain
an x64 `bashkit/_bashkit.pyd` with an empty PE certificate table: neither has an
embedded Authenticode signature. Windows signature checks above additionally
show any catalog-based trust available on that machine.

| Version | Wheel SHA-256 | `_bashkit.pyd` SHA-256 |
|---|---|---|
| 0.20.0 | `4bcee1e7bf4bed6b55d80c04694c95f1116685d847fbd9bfce42f76740e7915c` | `3d1ff960bba0ad3d24934622ef298e0feede430091c2a8214de0c7ab06170354` |
| 0.20.1 | `948f3f73a3e1289caeba7581330732fd145d5b26e5c018d97345c1bd92ce9099` | `e84d96745a00a6966089e164eb9cab8527ffb9a6094affbe5c4583b4c2e30e80` |

Sources: [0.20.0 release metadata](https://pypi.org/pypi/bashkit/0.20.0/json),
[0.20.1 release metadata](https://pypi.org/pypi/bashkit/0.20.1/json).
For both wheel filenames, PyPI's
`/integrity/bashkit/{version}/{filename}/provenance` endpoint returned HTTP 404
at inspection time; no PyPI-hosted attestation was available there. The
`publish-python.yml` files at the two release tags are identical and use OIDC
trusted publishing without an Authenticode signing step. Publisher authentication,
artifact attestations, and Windows code signing are separate mechanisms.

These observations do not establish why one binary was allowed and the other
blocked. Microsoft's [SAC FAQ](https://support.microsoft.com/en-us/windows/smart-app-control-frequently-asked-questions-285ea03d-fa88-4d56-882e-6698afdb7003)
explains that unsigned apps can be allowed through cloud reputation. Different
binary reputation is a hypothesis until the affected machine's events confirm
the policy decision. The SAC failure has not been reproduced in the Linux
artifact-inspection environment.

The durable packaging change is to Authenticode-sign the Windows `.pyd` and any
bundled DLLs with a trusted code-signing identity and timestamp, then verify
their signatures. Sign before final wheel packing, or unpack/sign/repack while
regenerating wheel `RECORD` hashes. Install that exact wheel on a SAC-On Windows
11 machine and test native import, execution, and the affected pytest collection
before publishing a new patch version. Do not replace existing PyPI artifacts.
Use Microsoft's [SAC signing guidance](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/code-signing-for-smart-app-control)
to select a trusted signing provider. A self-signed certificate or a PyPI
attestation alone does not supply that trust. Signing credentials and a SAC-On
validation host are required before claiming this fix is shipped.

The release workflow's signing path uses Azure Artifact Signing with a validated
Public Trust profile. It keeps unsigned wheels separate from publishable
artifacts, signs native files in a dedicated job, regenerates `RECORD`, and
verifies native signatures from the final wheel. A failure blocks publishing.
Windows CI also tests the packaging and signature checks without production
credentials; those tests do not replace the SAC-On comparison above. The
reported pin remains the workaround until a new signed release has passed that
comparison on the affected machine.

## Next steps

- [Sandbox configuration & limits](configuration.md), resource limits and sandbox options.
- [LLM tools](llm-tools.md), expose Bashkit as a sandboxed tool for agent frameworks.
- [Security](security.md), sandbox boundaries and what scripts cannot do.
