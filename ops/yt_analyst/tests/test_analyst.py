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

_RULES = {"summary": "S", "tutorial": "TUT", "rank": "RANK-RULES", "compare_extract": "CMP-RULES",
          "research": "RESEARCH-RULES", "product_score": "PRODUCT-RULES"}

def test_build_prompt_rank_uses_rank_rule():
    p = build_prompt({"mode": "rank", "depth": "medium", "custom": None}, transcript="T", rules=_RULES)
    assert "RANK-RULES" in p and "CMP-RULES" not in p and "TUT" not in p

def test_build_prompt_compare_extract_uses_compare_rule():
    p = build_prompt({"mode": "compare-extract", "depth": "medium", "custom": None}, transcript="T", rules=_RULES)
    assert "CMP-RULES" in p

def test_build_prompt_auto_and_summary_use_summary_rule():
    for mode in ("auto", "summary"):
        p = build_prompt({"mode": mode, "depth": "medium", "custom": None}, transcript="T", rules=_RULES)
        assert "S" in p and "RANK-RULES" not in p and "CMP-RULES" not in p

def test_build_prompt_custom_overrides_rank():
    p = build_prompt({"mode": "rank", "depth": "quick", "custom": "ONLY THIS"}, transcript="T", rules=_RULES)
    assert "ONLY THIS" in p and "RANK-RULES" not in p

def test_build_prompt_research_uses_research_rule():
    p = build_prompt({"mode": "research", "depth": "medium", "custom": None}, transcript="T", rules=_RULES)
    assert "RESEARCH-RULES" in p and "PRODUCT-RULES" not in p and "RANK-RULES" not in p

def test_build_prompt_product_score_uses_product_rule():
    p = build_prompt({"mode": "product-score", "depth": "medium", "custom": None}, transcript="T", rules=_RULES)
    assert "PRODUCT-RULES" in p and "RESEARCH-RULES" not in p and "S" in p

def test_build_prompt_custom_overrides_research_and_product():
    for mode in ("research", "product-score"):
        p = build_prompt({"mode": mode, "depth": "quick", "custom": "ONLY THIS"}, transcript="T", rules=_RULES)
        assert "ONLY THIS" in p and "RESEARCH-RULES" not in p and "PRODUCT-RULES" not in p
