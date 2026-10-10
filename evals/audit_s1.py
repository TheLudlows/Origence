"""Independently audit frozen S1 raw responses and rendered citation coverage."""
import argparse
import hashlib
import json
import math
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--run',required=True)
parser.add_argument('--dataset',default='evals/s1/v2')
args=parser.parse_args()
root=Path(args.run)
dataset=Path(args.dataset)
frozen=json.loads((dataset/'manifest.json').read_text())
for name,digest in frozen['sha256'].items():
    assert hashlib.sha256((dataset/name).read_bytes()).hexdigest()==digest,name
gold={c['id']:c for c in map(json.loads,(dataset/'cases.jsonl').read_text().splitlines())}
configs={'keyword':('keyword',False,False),'vector':('vector',False,False),'hybrid-original':('hybrid',False,False),'hybrid-summary':('hybrid',True,False),'hybrid-graph':('hybrid',False,True),'hybrid-all':('hybrid',True,True)}
mapping={r['asset_id']:r['source_id'] for r in json.loads((root/'imports.json').read_text())}
corpus={d['id']:d for d in map(json.loads,(dataset/'corpus.jsonl').read_text().splitlines())}
assert len(mapping)==36 and set(mapping.values())==set(corpus)
comparison=json.loads((root/'comparison.json').read_text())
checks={}
for name,(mode,summaries,graph) in configs.items():
    rows=list(map(json.loads,(root/name/'results.jsonl').read_text().splitlines()))
    assert len(rows)==300 and {r['id'] for r in rows}==set(gold)
    metrics=[]
    scored=[]
    manifest=json.loads((root/name/'manifest.json').read_text())
    assert manifest['k']==5 and manifest['budget_bytes']==2000
    assert manifest['corpus_sha256']==frozen['sha256']['corpus.jsonl']
    assert manifest['cases_sha256']==frozen['sha256']['cases.jsonl']
    for r in rows:
        assert r['status']=='ok'
        case=gold[r['id']];response=r['response'];hits=response['hits']
        for hit in hits:
            assert hit['version']==corpus[mapping[hit['asset_id']]]['source_version']
        assert response['effective_mode']==mode
        assert response['active_components']=={'summaries':summaries,'graph':graph}
        if not graph:assert response['graph']=={'entities':[],'relations':[]}
        ranked=list(dict.fromkeys(mapping[h['asset_id']] for h in hits))[:5]
        assert ranked==r['ranked_source_ids']
        relevant=set(case['relevant_source_ids'])
        if not relevant:
            assert r['false_positive']==bool(ranked)
            continue
        recall=len(relevant.intersection(ranked))/len(relevant)
        ranks=[i+1 for i,x in enumerate(ranked) if x in relevant]
        mrr=1/min(ranks) if ranks else 0
        dcg=sum(1/math.log2(i+1) for i in ranks)
        ndcg=dcg/sum(1/math.log2(i+2) for i in range(min(5,len(relevant))))
        evidence=case['evidence']
        found=sum(any(mapping[h['asset_id']]==e['source_id'] and h['version']==e['source_version'] and e['quote'] in h['content'] for h in hits if mapping[h['asset_id']] in ranked) for e in evidence)/len(evidence)
        assert r['resolve_status']=='ok'
        context=r['context'];text=context['rendered_context'];assert len(text.encode())<=2000 and len(text.encode())==context['count']
        assert context['active_components']==response['active_components']
        if not graph:assert not any('entity_id' in x or 'relation_id' in x for x in context['sources'])
        # Independently check actual cited context blocks, not graph provenance.
        citations=[s for s in context['sources'] if 'asset_id' in s]
        supported=0
        for e in evidence:
            for c in citations:
                if mapping[c['asset_id']]!=e['source_id'] or c['version']!=e['source_version']:continue
                marker=c['citation'];start=text.index(marker)
                next_positions=[text.find(s['citation'],start+len(marker)) for s in context['sources'] if isinstance(s.get('citation'),str)]
                ends=[p for p in next_positions if p>=0]
                block=text[start:min(ends) if ends else len(text)]
                if e['quote'] in block:supported+=1;break
        coverage=supported/len(evidence)
        for key,value in [('recall',recall),('mrr',mrr),('ndcg',ndcg),('evidence_recall',found),('coverage',coverage)]:
            assert abs(r[key]-value)<1e-12,(name,r['id'],key,r[key],value)
        metrics.append((recall,mrr,ndcg,found,coverage))
        scored.append((case['split'],metrics[-1]))
    for i,key in enumerate(['document_recall_at_k','mrr_at_k','ndcg_at_k','evidence_recall_at_k','coverage_at_budget']):
        value=sum(v[i] for v in metrics)/len(metrics);assert abs(comparison[name][key]-value)<1e-12
    for split in ['development','heldout']:
        subset=[values for label,values in scored if label==split]
        expected=comparison[name]['by_split'][split]
        for i,key in enumerate(['document_recall_at_k','mrr_at_k','ndcg_at_k','evidence_recall_at_k','coverage_at_budget']):
            assert abs(expected[key]-sum(v[i] for v in subset)/len(subset))<1e-12,(name,split,key)
        negatives=[r for r in rows if r['split']==split and not r['relevant_source_ids']]
        assert expected['no_answer_false_positives']==sum(bool(r['ranked_source_ids']) for r in negatives)
    assert comparison[name]['no_answer_false_positives']==sum(bool(r['ranked_source_ids']) for r in rows if not r['relevant_source_ids'])
    assert comparison[name]['fully_covered_at_budget']==sum(v[4]==1 for v in metrics)/len(metrics)
    checks[name]={'cases':len(rows),'answerable':len(metrics),'errors':0,'independent_metric_recalculation':'all per-case and aggregate values match','disabled_components':'verified on actual search/resolve responses'}
(root/'audit.json').write_text(json.dumps(checks,indent=2)+'\n')
print(json.dumps(checks,indent=2))
