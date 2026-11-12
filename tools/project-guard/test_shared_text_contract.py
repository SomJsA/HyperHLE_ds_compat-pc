import unittest
from guard import GuardError, shared_text_contract


class SharedTextContractTest(unittest.TestCase):
    def setUp(self):
        self.values = {'pc': {'common': 'same', 'input': 'keys'},
                       'android': {'common': 'same', 'input': 'motion'}}
        self.overrides = {'pc': {'input': 'keys'}, 'android': {'input': 'motion'}}

    def test_only_explicit_platform_input_difference_is_accepted(self):
        self.assertEqual(shared_text_contract(self.values, self.overrides)['shared_entries'], 1)

    def test_shared_drift_cannot_be_hidden_by_an_input_override(self):
        self.values['pc']['common'] = 'unexpected'
        with self.assertRaises(GuardError):
            shared_text_contract(self.values, self.overrides)

    def test_override_value_and_both_platforms_are_required(self):
        with self.assertRaises(GuardError):
            shared_text_contract(self.values, {'pc': self.overrides['pc']})
        self.overrides['android']['input'] = 'changed'
        with self.assertRaises(GuardError):
            shared_text_contract(self.values, self.overrides)
