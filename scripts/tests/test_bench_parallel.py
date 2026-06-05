import os
import stat
import subprocess
import textwrap
from pathlib import Path


def make_workspace(tmp_path: Path) -> Path:
    repo = tmp_path / "repo"
    (repo / "scripts").mkdir(parents=True)
    (repo / "crates/bashkit/benches/results").mkdir(parents=True)
    script = repo / "scripts/bench-parallel.sh"
    script.write_text(Path("scripts/bench-parallel.sh").read_text())
    script.chmod(script.stat().st_mode | stat.S_IXUSR)
    return repo


def make_fake_cargo(tmp_path: Path) -> Path:
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    cargo = bin_dir / "cargo"
    cargo.write_text(
        textwrap.dedent(
            """\
            #!/usr/bin/env bash
            cat <<'EOF'
            workload_types/light_sequential
                              time:   [10.000 ms 11.000 ms 12.000 ms]
            workload_types/light_parallel
                              time:   [5.000 ms 6.000 ms 7.000 ms]
            parallel_scaling/medium_seq/10
                              time:   [20.000 ms 21.000 ms 22.000 ms]
            parallel_scaling/medium_par/10
                              time:   [10.000 ms 11.000 ms 12.000 ms]
            single_parse
                              time:   [1.000 ms 2.000 ms 3.000 ms]
            EOF
            """
        )
    )
    cargo.chmod(cargo.stat().st_mode | stat.S_IXUSR)
    return bin_dir


def run_script(repo: Path, cache: Path, bin_dir: Path, *args: str) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env.update(
        {
            "PATH": f"{bin_dir}:{env['PATH']}",
            "XDG_CACHE_HOME": str(cache),
            "HOME": str(repo / "home"),
        }
    )
    return subprocess.run(
        [str(repo / "scripts/bench-parallel.sh"), *args],
        cwd=repo,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )


def test_bench_parallel_cache_uses_private_cache_not_shared_tmp(tmp_path: Path) -> None:
    repo = make_workspace(tmp_path)
    bin_dir = make_fake_cargo(tmp_path)
    cache = tmp_path / "cache"
    fixed_tmp = Path("/tmp/criterion-output.txt")
    if fixed_tmp.exists() or fixed_tmp.is_symlink():
        fixed_tmp.unlink()

    run_script(repo, cache, bin_dir)

    assert not fixed_tmp.exists()
    output_file = cache / "bashkit/criterion-parallel-output.txt"
    assert output_file.is_file()
    assert not output_file.is_symlink()
    assert stat.S_IMODE((cache / "bashkit").stat().st_mode) == 0o700
    assert stat.S_IMODE(output_file.stat().st_mode) == 0o600


def test_bench_parallel_dry_reads_private_cache(tmp_path: Path) -> None:
    repo = make_workspace(tmp_path)
    bin_dir = make_fake_cargo(tmp_path)
    cache = tmp_path / "cache"

    run_script(repo, cache, bin_dir)
    completed = run_script(repo, cache, bin_dir, "--dry")

    assert f"Using cached output from {cache}/bashkit/criterion-parallel-output.txt" in completed.stdout
    markdown = next((repo / "crates/bashkit/benches/results").glob("criterion-parallel-*.md"))
    assert "| light | 11.000 ms | 6.000 ms | **1.83x** |" in markdown.read_text()
