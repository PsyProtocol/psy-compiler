"""Check the complete Genesis recipe without building or generating artifacts."""
import os
from pathlib import Path
import shlex
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[1]
CONTRACTS = {
    "PsyTokenContractRef": "token",
    "USDTTokenContractRef": "usdt_token",
    "PsyDepositTreeContractRef": "deposit_tree",
    "PsyWithdrawalTreeContractRef": "withdrawal_tree",
    "PsyFaucetContractRef": "faucet",
    "MultisigPolicyContractRef": "multisig_policy",
}


class GenesisRecipeTests(unittest.TestCase):
    def test_full_compiled_contract_methods_match_exported_abis(self):
        environment = dict(os.environ, PWD=str(ROOT))
        result = subprocess.run(
            ["make", "--no-print-directory", "-n", "gen-deploy-json", "PSY_NETWORK=testnet"],
            cwd=ROOT, env=environment, check=True, capture_output=True, text=True,
        )
        compiled, exported = {}, {}
        for line in result.stdout.splitlines():
            if "--method-names" not in line:
                continue
            args = shlex.split(line)
            if "--method-names" not in args:
                continue
            methods = args[args.index("--method-names") + 1:]
            if "compile" in args:
                contract = next(arg.split("=", 1)[1] for arg in args if arg.startswith("--contract-name="))
                compiled[contract] = methods
            elif "generate-abi" in args:
                exported[args[args.index("--abi-name") + 1]] = methods
        self.assertEqual(set(compiled), set(CONTRACTS))
        self.assertEqual(set(exported), set(CONTRACTS.values()))
        for contract, artifact in CONTRACTS.items():
            with self.subTest(artifact=artifact):
                self.assertEqual(set(compiled[contract]), set(exported[artifact]))
                self.assertEqual(len(exported[artifact]), len(set(exported[artifact])))
        self.assertIn("mint", compiled["PsyTokenContractRef"])
        self.assertIn("set_chain_root", compiled["PsyWithdrawalTreeContractRef"])


if __name__ == "__main__":
    unittest.main()
