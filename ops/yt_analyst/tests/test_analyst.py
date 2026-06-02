from yt_analyst.analyst import model_for, build_prompt

def test_model_for():
    assert model_for("quick") == "sonnet"
    assert model_for("medium") == "opus"
    assert model_for("comprehensive") == "opus"
    assert model_for("bogus") == "opus"

def test_build_prompt_custom_overrides():
    job = {"mode": "summary", "depth": "quick", "intent": "x", "custom": "JUST DO THIS"}
    p = build_prompt(job, transcript="T", rules={"summary": "S", "tutorial": "U"})
    assert "JUST DO THIS" in p and "T" in p

def test_build_prompt_tutorial_uses_rules_and_depth_intent():
    job = {"mode": "tutorial", "depth": "comprehensive", "intent": "set up X", "custom": None}
    p = build_prompt(job, transcript="TRANSCRIPT", rules={"summary": "S", "tutorial": "TUT-RULES"})
    assert "TUT-RULES" in p
    assert "comprehensive" in p
    assert "set up X" in p
    assert "TRANSCRIPT" in p
