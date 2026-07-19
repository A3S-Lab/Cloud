#!/usr/bin/env python3

import argparse
import importlib.util
import pathlib
import sys
import unittest


MODULE_PATH = pathlib.Path(__file__).with_name("copy_oci_image.py")
SPEC = importlib.util.spec_from_file_location("copy_oci_image", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class ValidatedUrlTests(unittest.TestCase):
    def test_token_endpoint_accepts_an_https_path(self) -> None:
        self.assertEqual(
            MODULE.validated_url(
                "https://registry.example/auth/token",
                "token URL",
                {"https"},
                allow_path=True,
            ),
            "https://registry.example/auth/token",
        )

    def test_registry_origin_rejects_a_path(self) -> None:
        with self.assertRaises(argparse.ArgumentTypeError):
            MODULE.validated_url(
                "https://registry.example/v2",
                "source",
                {"https"},
            )

    def test_url_rejects_embedded_credentials(self) -> None:
        with self.assertRaises(argparse.ArgumentTypeError):
            MODULE.validated_url(
                "https://user:secret@registry.example",
                "source",
                {"https"},
            )


if __name__ == "__main__":
    unittest.main()
