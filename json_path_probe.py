import sqlite3

conn = sqlite3.connect(":memory:")
cases = [
    (
        "attr_dollar_quoted",
        "select json_extract('{\"gen_ai.system\":\"anthropic\"}', '$.\"gen_ai.system\"')",
    ),
    (
        "attr_bracket_quoted",
        "select json_extract('{\"gen_ai.system\":\"anthropic\"}', '$[\"gen_ai.system\"]')",
    ),
    (
        "attr_single_quoted",
        "select json_extract('{\"gen_ai.system\":\"anthropic\"}', '$[''gen_ai.system'']')",
    ),
    (
        "res_dollar_quoted",
        "select json_extract('{\"attributes\":{\"service.name\":\"gateway\"}}', '$.attributes.\"service.name\"')",
    ),
    (
        "res_bracket_quoted",
        "select json_extract('{\"attributes\":{\"service.name\":\"gateway\"}}', '$.attributes[\"service.name\"]')",
    ),
    (
        "res_single_quoted",
        "select json_extract('{\"attributes\":{\"service.name\":\"gateway\"}}', '$.attributes[''service.name'']')",
    ),
]

for name, sql in cases:
    try:
        print(name, conn.execute(sql).fetchone()[0])
    except Exception as exc:
        print(name, "ERROR", exc)
