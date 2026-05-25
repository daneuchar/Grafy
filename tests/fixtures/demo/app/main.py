"""Entry point. ``alert`` calls the *aliased re-export* ``lib.send``.

A binding-precise resolver (scip-python) will follow the alias through
``lib/__init__.py`` and report the call target as ``lib.notify.send_email``.
A heuristic resolver that walks imports + names without typing information
will either fail to resolve (no edge) or resolve to the alias itself
(``lib.send``), which is the wrong definition site.
"""

from lib import send


class User:
    def __init__(self, email: str) -> None:
        self.email = email


def alert(user: User, msg: str) -> None:
    send(user.email, msg)
