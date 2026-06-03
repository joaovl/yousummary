"""Prompt assembly + Claude CLI invocation for the analysis worker."""
from __future__ import annotations
import subprocess

_MODEL = {"quick": "sonnet", "medium": "opus", "comprehensive": "opus"}


def model_for(depth: str) -> str:
    return _MODEL.get(depth, "opus")


def build_prompt(job: dict, transcript: str, rules: dict) -> str:
    """Assemble the worker prompt. Custom instructions override the structured mode."""
    depth = job.get("depth", "medium")
    intent = (job.get("intent") or "").strip()
    custom = (job.get("custom") or "").strip()
    intent_line = f"\nUser intent: {intent}\n" if intent else ""
    if custom:
        head = f"Follow these instructions exactly:\n{custom}\n"
    else:
        mode = job.get("mode", "auto")
        key = {"tutorial": "tutorial", "rank": "rank", "compare-extract": "compare_extract", "research": "research", "product-score": "product_score"}.get(mode, "summary")
        rule = rules.get(key, rules.get("summary", ""))
        head = f"{rule}\nDepth: {depth}.{intent_line}"
    return f"{head}\n\n--- TRANSCRIPT ---\n{transcript}\n--- END TRANSCRIPT ---\n"


def run_claude(prompt: str, model: str, allow_web: bool, timeout: int = 600) -> str:
    """Invoke the Max-sub Claude CLI. Read-only web tools when allow_web."""
    tools = "WebFetch WebSearch" if allow_web else ""
    cmd = ["claude", "-p", "--output-format", "text", "--model", model]
    if tools:
        cmd += ["--allowedTools", tools]
    p = subprocess.run(cmd, input=prompt, capture_output=True, text=True, timeout=timeout)
    if p.returncode != 0:
        raise RuntimeError(f"claude rc={p.returncode}: {(p.stderr or '').strip()[:300]}")
    out = (p.stdout or "").strip()
    if not out:
        raise RuntimeError("claude produced no output")
    return out
