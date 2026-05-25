"""Public re-export: ``send`` is an alias for ``send_email`` in this package."""

from .notify import send_email as send

__all__ = ["send"]
