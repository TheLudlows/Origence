"""Common deterministic budgets using the pinned BGE reference tokenizer."""
import json
from aml_drill import DrillError
from aml_experiment import evidence_payload


class Budget:
    def __init__(self, tokenizer_file):
        from tokenizers import Tokenizer
        self.tokenizer = Tokenizer.from_file(str(tokenizer_file))
        self.tokenizer.no_truncation()
        self.tokenizer.no_padding()

    def size(self, hits):
        text = json.dumps(evidence_payload(hits), ensure_ascii=False)
        return {"reference_tokens": len(self.tokenizer.encode(text, add_special_tokens=False).ids),
                "utf8_bytes": len(text.encode())}

    def pack(self, hits, count, tokens, byte_limit):
        output = []
        for hit in hits[:count]:
            size = self.size(output + [hit])
            if size["reference_tokens"] > tokens or size["utf8_bytes"] > byte_limit:
                break
            output.append(hit)
        return output, self.size(output)


def checked_order(indices, count):
    if (not isinstance(indices, list) or len(indices) != count or
            any(type(i) is not int for i in indices) or set(indices) != set(range(count))):
        raise DrillError("invalid_complete_ranking")
    return indices
