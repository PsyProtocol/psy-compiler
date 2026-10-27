"""Check expanded release recipes without executing Cargo or artifact writers."""

import os
from pathlib import Path
import shlex
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[1]
WITHDRAWAL_METHODS = {
    "set_chain_root", "get_root", "get_chain_root", "append_leaf",
    "append_withdrawal", "batch_append_withdrawals_2", "batch_append_withdrawals_5",
}


class GenesisMakeTargetsTests(unittest.TestCase):
    def check_withdrawal_methods(self, *targets):
        env = dict(os.environ)
        for name in ("MAKEFLAGS", "GNUMAKEFLAGS", "MFLAGS", "MAKEFILES"):
            env.pop(name, None)
        command = ["make", "--no-print-directory", "-n", "-f", "Makefile",
                   f"PWD={ROOT}", "PSY_NETWORK=testnet", *targets]
        result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True,
                                text=True, timeout=30, check=True)
        found = {"compile": [], "generate-abi": []}
        for line in result.stdout.splitlines():
            if "PsyWithdrawalTreeContractRef" not in line:
                continue
            words = shlex.split(line)
            for action in found:
                if action not in words:
                    continue
                self.assertIn("--method-names", words)
                methods = words[words.index("--method-names") + 1:]
                found[action].append(methods)
                print(f"{' '.join(targets)}: {action}: {' '.join(methods)}", flush=True)
        for action, invocations in found.items():
            self.assertEqual(len(invocations), 1, f"expected one withdrawal {action} recipe")
            methods = invocations[0]
            self.assertEqual(len(methods), len(set(methods)), f"duplicate {action} method")
        self.assertEqual(set(found["compile"][0]), set(found["generate-abi"][0]),
                         "withdrawal compile and published ABI methods must agree")
        self.assertEqual(set(found["compile"][0]), WITHDRAWAL_METHODS)

    def test_full_genesis_withdrawal_compile_and_abi_methods_agree(self):
        self.check_withdrawal_methods("gen-deploy-json")

    def test_standalone_withdrawal_compile_and_abi_methods_agree(self):
        self.check_withdrawal_methods("compile-withdrawal-tree-contract", "compile-withdrawal-tree-abi")


if __name__ == "__main__":
    unittest.main()
