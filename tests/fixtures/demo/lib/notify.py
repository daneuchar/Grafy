"""Concrete sender for the demo. The function is `lib.notify.send_email`."""


def send_email(addr: str, body: str) -> None:
    """Send an email to ``addr`` with ``body``. Prints to stdout in the demo."""
    print(f"to={addr} body={body}")
