#!/usr/bin/env python3
"""Send the overnight run's messages, and nothing else.

This file only sends. It talks SMTP, which is for sending mail, to one
server, and has no code that reads, lists or deletes mail. It does not
control Mail.app or any other program.

Settings, from the environment (put them in the launchd agent):

    QUILL_OVERNIGHT_MAIL   the address to send to and from
    QUILL_OVERNIGHT_SMTP   the server, default smtp.mail.me.com (iCloud)

The password is an app-specific password the person makes for this (for
iCloud, at appleid.apple.com), kept in their login keychain under the
service name "quill-overnight-mail" and the address as the account:

    security add-generic-password -s quill-overnight-mail -a ADDRESS -T /usr/bin/security -w

(`-w` with nothing after it asks for the password, so it is never typed on
the command line.) Revoking that password at appleid.apple.com ends the
messages at once.

Standard library only.
"""

from __future__ import annotations

from email.message import EmailMessage
import os
import smtplib
import ssl
import subprocess

SERVICE = "quill-overnight-mail"
PORT = 587


def address() -> str:
    return os.environ.get("QUILL_OVERNIGHT_MAIL", "").strip()


def password(account: str) -> str | None:
    """The app-specific password from the login keychain, or None."""
    p = subprocess.run(["/usr/bin/security", "find-generic-password", "-s", SERVICE,
                        "-a", account, "-w"], capture_output=True, text=True)
    return p.stdout.strip() if p.returncode == 0 and p.stdout.strip() else None


def send(title: str, body: str) -> str | None:
    """Send one message from the address to itself. None when it was sent or
    when no address is set; otherwise why it was not sent."""
    to = address()
    if not to:
        return None
    secret = password(to)
    if not secret:
        return f'no password in the keychain under "{SERVICE}" for {to}'
    message = EmailMessage()
    message["From"] = to
    message["To"] = to
    message["Subject"] = title
    message.set_content(body)
    server = os.environ.get("QUILL_OVERNIGHT_SMTP", "smtp.mail.me.com")
    try:
        with smtplib.SMTP(server, PORT, timeout=60) as smtp:
            smtp.starttls(context=ssl.create_default_context())
            smtp.login(to, secret)
            smtp.send_message(message)
    except (OSError, smtplib.SMTPException) as error:
        return f"{server}: {error}"
    return None
