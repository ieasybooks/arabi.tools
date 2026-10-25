"""Write the production Compose environment to stdout, which the workflow pipes over SSH."""
import json
import os


def quote(value):
    """Compose single quotes keep dollar signs and JSON backslash escapes literal."""
    if "\n" in value or "\r" in value:
        raise ValueError("environment values must fit on one line")
    return "'" + value.replace("'", "\\'") + "'"


def render(environment):
    # Normalize layout, not the keys' contents. Escaping apostrophes as JSON unicode avoids
    # any interaction between JSON string escaping and Compose's single-quote escaping.
    accounts = json.loads(environment["DRIVE_SERVICE_ACCOUNTS"])
    if not isinstance(accounts, list) or not accounts:
        raise ValueError("DRIVE_SERVICE_ACCOUNTS must contain a nonempty JSON array")
    accounts = json.dumps(accounts, ensure_ascii=True, separators=(",", ":")).replace("'", "\\u0027").replace("$", "\\u0024")
    return "\n".join([
        "WIT_TOKENS_AR=" + quote(environment["WIT_TOKENS_AR"]),
        "TUNNEL_TOKEN=" + quote(environment["TUNNEL_TOKEN"]),
        "DRIVE_SERVICE_ACCOUNTS=" + quote(accounts),
        "",
    ])


if __name__ == "__main__":
    try:
        output = render(os.environ)
    except (KeyError, ValueError):
        # JSON parser exceptions can contain snippets of input; never print them.
        raise SystemExit("Invalid production environment; check the required settings.") from None
    print(output, end="")
