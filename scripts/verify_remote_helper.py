#!/usr/bin/env python3
"""Exercise a built helper over SSH using isolated fixtures, then read live data.

Usage: python3 scripts/verify_remote_helper.py --host HOST --binary REMOTE_PATH --output FILE
Uses existing SSH config/authentication. The binary must already be on the host.
"""
import argparse
import json
import select
import shlex
import subprocess


def ssh(host, args):
    return ["ssh", "-T", "-o", "ClearAllForwardings=yes", "-o", "BatchMode=yes",
            "-o", "StrictHostKeyChecking=yes", "-o", "ConnectTimeout=10",
            host, shlex.join(args)]


def remote_python(host, source):
    result = subprocess.run(ssh(host, ["python3", "-c", source]),
                            capture_output=True, text=True, timeout=40, check=True)
    return json.loads(result.stdout)


class Session:
    def __init__(self, host, binary, home=None, writable=False):
        args = [binary, "serve"]
        if home:
            args += ["--sandbox-home", home]
        if writable:
            args += ["--allow-writes"]
        self.process = subprocess.Popen(ssh(host, args), stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.counter = 0

    def call(self, method, params=None, expect_error=False):
        self.counter += 1
        request = {"id": str(self.counter), "version": 1, "method": method, "params": params}
        self.process.stdin.write((json.dumps(request) + "\n").encode())
        self.process.stdin.flush()
        ready, _, _ = select.select([self.process.stdout], [], [], 30)
        if not ready:
            raise TimeoutError(method)
        response = json.loads(self.process.stdout.readline())
        assert response["id"] == request["id"], response
        if expect_error:
            assert "error" in response, response
            return response["error"]
        if "error" in response:
            raise RuntimeError(response["error"])
        return response["result"]

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        error = self.process.stderr.read().decode(errors="replace")
        self.process.stdout.close()
        self.process.stderr.close()
        if not exc_type and self.process.returncode:
            raise RuntimeError(error)


SNAPSHOT = """
import hashlib,json,os
from pathlib import Path
h=Path.home()
paths=[h/'.codex/config.toml',h/'.claude/settings.json',h/'.skill-studio/config.json']
for root in [h/'.codex/skills',h/'.claude/skills',h/'.agents/skills']:
 if root.is_dir(): paths.extend(p for p in root.rglob('*') if p.is_file())
print(json.dumps({str(p):hashlib.sha256(p.read_bytes()).hexdigest() if p.is_file() else None for p in paths}))
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    if args.host.startswith("-") or not args.binary.startswith("/"):
        parser.error("host must not start with '-' and binary must be absolute")
    before = remote_python(args.host, SNAPSHOT)
    fixture = remote_python(args.host, """
import tempfile,json
from pathlib import Path
h=Path(tempfile.mkdtemp(prefix='skill-studio-helper-fixture-'))
for agent in ['.claude','.codex']:
 p=h/agent/'skills'/'remote-probe'
 p.mkdir(parents=True)
 (p/'SKILL.md').write_text('---\\nname: remote-probe\\ndescription: isolated test\\n---\\nProbe\\n')
(h/'.claude/settings.json').write_text('{"keep":"fixture"}')
(h/'.codex/config.toml').write_text('# preserved\\nmodel = "fixture"\\n')
print(json.dumps(str(h)))
""")
    report = {"host": args.host, "binary": args.binary, "fixture": fixture}
    try:
        with Session(args.host, args.binary, fixture, True) as session:
            report["hello"] = session.call("hello")
            assert report["hello"]["os"] == "linux"
            agents = session.call("list_agents")
            assert len(agents) == 2 and all(a["detected"] for a in agents)
            skills = session.call("scan_skills")
            ids = {agent: next(s["id"] for s in skills if s["agents"][agent]["status"] == "source")
                   for agent in ("claude-code", "codex")}
            for agent, skill_id in ids.items():
                for _ in range(2):
                    session.call("set_skill_enabled", {"agentId": agent, "skillId": skill_id, "enabled": False})
            report["disable_and_repeat"] = "passed"
        with Session(args.host, args.binary, fixture, True) as session:
            skills = session.call("scan_skills")
            for agent, skill_id in ids.items():
                skill = next(s for s in skills if s["id"] == skill_id)
                assert skill["agents"][agent]["disabled"]
                session.call("set_skill_enabled", {"agentId": agent, "skillId": skill_id, "enabled": True})
            for skill in session.call("scan_skills"):
                for state in skill["agents"].values():
                    if state["status"] == "source":
                        assert not state["disabled"]
            report["reconnect_and_enable"] = "passed"
        report["unrelated_config_preserved"] = remote_python(args.host, f"""
import json
from pathlib import Path
h=Path({fixture!r})
assert json.loads((h/'.claude/settings.json').read_text())['keep']=='fixture'
assert (h/'.codex/config.toml').read_text().startswith('# preserved\\nmodel = "fixture"')
print(json.dumps(True))
""")
        with Session(args.host, args.binary) as session:
            hello = session.call("hello")
            assert not hello["writable"]
            agents = session.call("list_agents")
            skills = session.call("scan_skills")
            report["live"] = {"home": hello["home"],
                              "agents": [{"id": a["id"], "detected": a["detected"], "configDir": a["configDir"]} for a in agents],
                              "skills": [{"id": s["id"], "name": s["name"], "sourcePath": s["sourcePath"]} for s in skills]}
            session.call("set_skill_enabled", {}, expect_error=True)
            report["readonly_write_rejected"] = True
        after = remote_python(args.host, SNAPSHOT)
        report["live_files_unchanged"] = before == after
        assert report["live_files_unchanged"]
        report["passed"] = True
    except Exception as exc:
        report["error"] = str(exc)
        raise
    finally:
        with open(args.output, "w") as output:
            json.dump(report, output, indent=2, ensure_ascii=False)
            output.write("\n")
    print(json.dumps(report, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
