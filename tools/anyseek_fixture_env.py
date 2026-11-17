"""Pass only an explicitly provisioned loopback test peer through smoke isolation."""
import os
from urllib.parse import urlsplit


def configure_fixture(env):
    names = ("URL", "API_KEY", "COLLECTION")
    values = {name: os.environ.get("ORIGENCE_TEST_ANYSEEK_" + name) for name in names}
    if not any(values.values()):
        return env
    if not all(values.values()):
        raise RuntimeError("incomplete AnySeek test fixture configuration")
    url = urlsplit(values["URL"])
    if url.scheme != "http" or url.hostname not in {"127.0.0.1", "localhost", "::1"} or url.username or url.password or url.path not in {"", "/"} or url.query or url.fragment:
        raise RuntimeError("AnySeek smoke fixture must be an uncredentialed loopback origin")
    env["OC_FULL_TEXT_BACKEND"] = "anyseek"
    env.update({"OC_ANYSEEK_" + name: value for name, value in values.items()})
    return env
