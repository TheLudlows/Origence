"""No runtime/dependency/network required; fixtures are not model quality evidence."""
import copy
import contextlib
import io
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))
import s4_agent as s4
import s4_local_chat as local


class ScriptModel:
    def __init__(self, actions):
        self.actions = iter(actions); self.inputs = []
    def complete(self, messages):
        self.inputs.append(copy.deepcopy(messages))
        action = next(self.actions)
        if isinstance(action, Exception): raise action
        text = action if isinstance(action, str) else json.dumps(action)
        return s4.ModelReply(text, {'content': text})


def retrieve(query='Vela'):
    return {'action': 'retrieve', 'query': query}


def finish(answer='', citations=None, answerable=False):
    return {'action': 'finish', 'answerable': answerable, 'answer': answer, 'citations': citations or []}


class S4Tests(unittest.TestCase):
    def setUp(self):
        self.corpus, self.cases, self.labels, self.config, self.freeze = s4.load_frozen()
        self.case = self.cases[0]
        self.fixture = s4.FixtureTools(self.corpus)

    def run_case(self, actions, arm='simple_hybrid', tools=None, config=None, case=None):
        model = ScriptModel(actions)
        row = s4.run_agent(case or self.case, arm, model, tools or self.fixture, config or self.config)
        return row, model

    def test_frozen_new_dataset_covers_routes_and_categories(self):
        self.assertEqual(len(self.cases), 10)
        self.assertEqual(set(self.config['arms']), set(s4.ARMS))
        self.assertEqual({d['route'] for d in self.corpus}, {'memory','knowledge','learning','session'})
        self.assertTrue({'release','handover','changed_decision','conditional','conflict','revoked_support','no_answer','context_scope'} <= {c['category'] for c in self.cases})

    def test_freeze_detects_tamper_before_inference(self):
        with tempfile.TemporaryDirectory() as directory:
            for name in ['freeze.json', *self.freeze['files']]:
                Path(directory, name).write_bytes((s4.DATA / name).read_bytes())
            Path(directory, 'cases.json').write_text('{}')
            with self.assertRaisesRegex(s4.HarnessError, 'frozen_input_changed'):
                s4.load_frozen(directory)

    def test_real_interaction_loop_sees_retrieval_before_next_action(self):
        citation = 's4:release-approval:v1'
        row, model = self.run_case([retrieve('Mira Chen Vela'), finish('Mira and Chen', [citation], True)])
        self.assertEqual(row['status'], 'completed')
        self.assertEqual((row['model_calls'],row['tool_calls']), (2,1))
        self.assertIn('Mira', model.inputs[1][-1]['content'])
        self.assertEqual([t['kind'] for t in row['trace']], ['model','tool','model'])

    def test_model_can_revise_query(self):
        row, model = self.run_case([retrieve('unknown-words'), retrieve('Mira Chen'), finish()])
        self.assertEqual(row['status'], 'completed')
        self.assertEqual(row['tool_calls'], 2)
        self.assertEqual(len(model.inputs), 3)

    def test_model_call_budget(self):
        config = dict(self.config, max_model_calls=2, max_tool_calls=10)
        row,_ = self.run_case([retrieve(), retrieve(), finish()],config=config)
        self.assertEqual(row['error'],'model_budget_exhausted')
        self.assertEqual(row['model_calls'],2)

    def test_tool_call_budget(self):
        config = dict(self.config,max_tool_calls=1)
        row,_ = self.run_case([retrieve(),retrieve(),finish()],config=config)
        self.assertEqual(row['error'],'tool_budget_exhausted')
        self.assertEqual(row['tool_calls'],1)

    def test_context_budget_prevents_model_request(self):
        row,model = self.run_case([finish()],config=dict(self.config,max_context_bytes=10))
        self.assertEqual(row['error'],'context_budget_exhausted')
        self.assertEqual(model.inputs,[])

    def test_small_evidence_budget_never_truncates_sources(self):
        row,model = self.run_case([retrieve(),finish()],config=dict(self.config,max_evidence_bytes=1))
        self.assertEqual(row['delivered_evidence'],[])
        self.assertEqual(row['evidence_bytes'],0)
        self.assertIn('omitted_count',model.inputs[-1][-1]['content'])

    def test_explicit_conflicts_pack_atomically(self):
        a=s4.source_evidence(self.corpus[0]);b=s4.source_evidence(self.corpus[1])
        a['conflict_id']=b['conflict_id']='conflict-1'
        accepted,omitted,size=s4.pack_evidence([a,b],{},len(s4.encoded(a))+1,12)
        self.assertEqual(accepted,[]);self.assertEqual(size,0);self.assertEqual(len(omitted),2)
        self.assertEqual(s4.pack_evidence([a,b],{},10000,1)[0],[])

    def test_duplicate_retrieval_not_charged_twice(self):
        row,_=self.run_case([retrieve('Mira'),retrieve('Mira'),finish()])
        evidence=row['delivered_evidence']
        self.assertEqual(len(evidence),1)
        self.assertEqual(row['evidence_bytes'],len(s4.encoded(row['trace'][1]['delivered'])))

    def test_malformed_responses_fail_closed(self):
        for content in ['bad', '[]', '{}', '{"action":"send_email"}', json.dumps({'action':'retrieve','query':''}),
                        json.dumps({'action':'finish','answerable':False,'answer':'guess','citations':[]}),
                        json.dumps({'action':'finish','answerable':True,'answer':'x','citations':[1]}),
                        json.dumps({'action':'finish','answerable':True,'answer':'x','citations':['x','x']})]:
            with self.subTest(content=content):
                row,_=self.run_case([content]);self.assertEqual(row['status'],'error');self.assertEqual(row['tool_calls'],0)

    def test_missing_citations_fail_success_without_crashing(self):
        row,_=self.run_case([finish('Mira Chen 21:00 22:00 Thursday 2 5',[],True)])
        score=s4.assess(row,self.labels[self.case['id']])
        self.assertTrue(score['missing_citations_for_answer'])
        self.assertFalse(score['task_success_heuristic'])
        self.assertIsNone(score['citation_source_support_heuristic'])
        self.assertEqual(score['required_source_coverage'],0)

    def test_unknown_citations_count_in_denominator(self):
        row,_=self.run_case([finish('guess',['invented'],True)])
        score=s4.assess(row,self.labels[self.case['id']])
        self.assertEqual(score['citation_source_support_heuristic_denominator'],1)
        self.assertEqual(score['citation_source_support_heuristic'],0)
        self.assertEqual(score['unknown_citations'],['invented'])

    def test_noanswer_errors_are_not_correct_abstentions(self):
        label=self.labels['s4-no-answer']
        row,_=self.run_case(['invalid'])
        score=s4.assess(row,label)
        self.assertFalse(score['abstained']);self.assertFalse(score['task_success_heuristic'])
        self.assertIsNone(score['required_source_coverage'])
        row,_=self.run_case([finish()]);self.assertTrue(s4.assess(row,label)['task_success_heuristic'])

    def test_no_memory_never_exposes_corpus(self):
        row,model=self.run_case([retrieve(),finish()],arm='no_memory')
        self.assertEqual(row['delivered_evidence'],[])
        self.assertNotIn('MAPLE-614',json.dumps(model.inputs))
        self.assertNotIn('Mira',json.dumps(model.inputs))

    def test_non_gold_tools_have_no_labels_and_score_after_generation(self):
        self.assertFalse(hasattr(self.fixture,'labels'))
        for arm in s4.ARMS[:-1]:
            row,model=self.run_case([retrieve(),finish()],arm=arm)
            for forbidden in ['answer_alias_groups','required_source_ids','rubric','forbidden_answer_substrings']:
                self.assertNotIn(forbidden,json.dumps(model.inputs))
            self.assertNotIn('metrics',row)

    def test_gold_contains_sources_but_not_answers_or_rubrics(self):
        row,model=self.run_case([retrieve(),finish()],arm='gold_diagnostic',tools=s4.GoldTools(self.corpus,self.labels))
        self.assertEqual({e['source_id'] for e in row['delivered_evidence']},{'release-approval','release-window'})
        self.assertNotIn('answer_alias_groups',json.dumps(model.inputs))
        self.assertNotIn('rubric',json.dumps(model.inputs))

    def test_revoked_source_not_delivered_even_if_query_matches(self):
        for arm in ['simple_hybrid','evidence_bundle']:
            rows,_=self.fixture.retrieve(arm,self.case,'MAPLE-614')
            self.assertEqual(rows,[])

    def test_bundle_observation_preserves_product_render_verbatim(self):
        class Bundle:
            def retrieve(inner,*args):
                a=s4.source_evidence(self.corpus[0]);a['evidence_id']='real-id'
                return [a],{'rendered_context':'CONFLICT precise product text\n[real-id] Mira and Chen\n','warnings':['scope unverified']}
        row,model=self.run_case([retrieve(),finish()],arm='evidence_bundle',tools=Bundle())
        text=model.inputs[1][-1]['content']
        self.assertIn('CONFLICT precise product text\n[real-id] Mira and Chen\n',text)
        self.assertNotIn('release-approval',text)
        self.assertEqual(row['delivered_evidence'][0]['evidence_id'],'real-id')

    def test_oversize_bundle_is_omitted_whole(self):
        class Bundle:
            def retrieve(inner,*args):
                return [s4.source_evidence(self.corpus[0])],{'rendered_context':'x'*500,'warnings':[]}
        row,model=self.run_case([retrieve(),finish()],arm='evidence_bundle',tools=Bundle(),config=dict(self.config,max_evidence_bytes=100))
        self.assertEqual(row['delivered_evidence'],[])
        self.assertNotIn('x'*100,model.inputs[1][-1]['content'])
        self.assertIn('omitted_bundle',model.inputs[1][-1]['content'])

    def test_loopback_prevents_remote_credentials_and_redirects(self):
        for url in ['https://127.0.0.1','http://localhost:80','http://example.com','http://user:pass@127.0.0.1','http://127.0.0.1/?key=secret','http://127.0.0.1/#x']:
            with self.subTest(url=url),self.assertRaises(s4.HarnessError):s4.require_loopback(url)
        self.assertEqual(s4.require_loopback('http://127.0.0.1:8099/'),'http://127.0.0.1:8099')
        with self.assertRaisesRegex(s4.HarnessError,'redirect_forbidden'):
            s4.NoRedirect().redirect_request(None,None,None,None,None,'http://example.com')

    def test_http_rejects_array_and_null_envelopes(self):
        for body in [b'[]',b'null',b'"text"']:
            with self.subTest(body=body):
                class Response:
                    def __enter__(inner):return inner
                    def __exit__(inner,*unused):pass
                    def read(inner,*unused):return body
                class Opener:
                    def open(inner,*unused,**kwargs):return Response()
                http=s4.LocalHTTP('http://127.0.0.1:1',self.config);http.opener=Opener()
                with self.assertRaisesRegex(s4.HarnessError,'http_json_object_required'):http.request('/v1/search',{})

    def test_malformed_adapter_shape_retains_failure(self):
        class Bad:
            def retrieve(inner,*args):return [],None
        row,_=self.run_case([retrieve()],tools=Bad())
        self.assertEqual(row['error'],'malformed_adapter_response')
        self.assertFalse(s4.assess(row,self.labels['s4-no-answer'])['task_success_heuristic'])

    def test_session_binding_includes_session_not_just_turn(self):
        a={'type':'session_turn','session_id':'one','turn_id':'same','source_event_id':'event'}
        b=dict(a,session_id='two')
        self.assertNotEqual(s4.OrigenceTools.binding_key(a),s4.OrigenceTools.binding_key(b))

    def test_simple_hybrid_never_enriches_from_frozen_oracle(self):
        tool=object.__new__(s4.OrigenceTools);tool.config=self.config;tool.last_raw=None
        doc=dict(self.corpus[0],conditions=['SECRET-LABEL-CONDITION'])
        tool.docs={doc['id']:doc}
        provenance={'asset_id':'a','asset_version':1,'chunk_id':'c','source_event_id':'e','source_version':1,'locator':{'actual':'locator'}}
        tool.binding={s4.OrigenceTools.binding_key(provenance):{'source_id':doc['id'],'source_event_id':'e','content_sha256':s4.sha(doc['content'].encode())}}
        class HTTP:
            def request(inner,*args):return {'hits':[dict(content=doc['content'],version=1,**{k:v for k,v in provenance.items() if k!='asset_version'})]}
        tool.http=HTTP()
        rows,_=tool.retrieve('simple_hybrid',self.case,'Vela')
        self.assertNotIn('SECRET-LABEL-CONDITION',json.dumps(rows))
        self.assertEqual(rows[0]['locator'],{'actual':'locator'})

    def product_tool(self, doc, item):
        tool=object.__new__(s4.OrigenceTools);tool.config=self.config;tool.last_raw=None;tool.docs={doc['id']:doc}
        p=item['provenance']
        tool.binding={s4.OrigenceTools.binding_key(p):{'source_id':doc['id'],'source_event_id':p['source_event_id'],
                    'content_sha256':s4.sha(item['content'].encode())}}
        class HTTP:
            def request(inner,*args):return {'items':[item],'rendered_context':item['content'],'conflicts':[]}
        tool.http=HTTP()
        return tool

    def test_product_session_exact_wrapper_and_binding(self):
        doc=next(d for d in self.corpus if d['route']=='session')
        item={'evidence_id':'turn:s:t','route':'session','content':doc['content'],
              'provenance':{'type':'session_turn','session_id':'s','turn_id':'t','source_event_id':'e','used_evidence':[]}}
        rows,_=self.product_tool(doc,item).retrieve('evidence_bundle',self.case,'Vela')
        self.assertEqual(rows[0]['content'],doc['content'])
        self.assertEqual(rows[0]['provenance']['type'],'session_turn')

    def test_product_learning_wrapper_preserved_not_replaced(self):
        doc=next(d for d in self.corpus if d['route']=='learning')
        content='Conditional learning\nConditions:\n'+'\n'.join(doc['conditions'])+'\n\nStatement:\n'+doc['content']+'\n\nWhy:\nThe validator found a staging mismatch.'
        item={'evidence_id':'chunk:c','route':'learning','content':content,'conditions':doc['conditions'],
              'provenance':{'type':'asset_chunk','asset_id':'a','asset_version':1,'chunk_id':'c','source_event_id':'e','source_version':1}}
        rows,_=self.product_tool(doc,item).retrieve('evidence_bundle',self.case,'Vela')
        self.assertEqual(rows[0]['content'],content)
        bad=copy.deepcopy(item);bad['content']='An invented learning without the frozen statement'
        with self.assertRaisesRegex(s4.HarnessError,'source_quote_mismatch'):
            self.product_tool(doc,bad).retrieve('evidence_bundle',self.case,'Vela')

    def test_simple_hybrid_keeps_original_member_source_version(self):
        doc=self.corpus[0]
        tool=object.__new__(s4.OrigenceTools);tool.config=self.config;tool.last_raw=None;tool.docs={doc['id']:doc}
        hit={'asset_id':'a','version':2,'chunk_id':'c','source_event_id':'e','content':doc['content'],'locator':{'source_version':1}}
        tool.binding={('asset_chunk','a',2,'c'):{'source_id':doc['id'],'source_event_id':'e','content_sha256':s4.sha(doc['content'].encode())}}
        class HTTP:
            def request(inner,*args):return {'hits':[hit]}
        tool.http=HTTP()
        rows,_=tool.retrieve('simple_hybrid',self.case,'Vela')
        self.assertEqual(rows[0]['provenance']['asset_version'],2)
        self.assertEqual(rows[0]['provenance']['source_version'],1)

    def test_generic_local_request_does_not_turn_json_into_graph(self):
        body={'model':'test','messages':[{'role':'user','content':'hello'}],'max_tokens':12,'temperature':0,
              'response_format':{'type':'json_schema','json_schema':{'schema':s4.ACTION_SCHEMA}}}
        messages,limit,schema=local.check_request(body,'test')
        self.assertEqual(messages,body['messages']);self.assertEqual(limit,12);self.assertEqual(schema,s4.ACTION_SCHEMA)
        self.assertNotIn('entities',schema['properties'])
        for change in [{'max_tokens':1025},{'temperature':1},{'messages':[{'role':'tool','content':'x'}]}, {'response_format':{'type':'json_object'}}]:
            with self.assertRaises(ValueError):local.check_request(dict(body,**change),'test')

    def test_oversize_model_usage_fails(self):
        class TooMuch:
            def complete(inner,messages):return s4.ModelReply(json.dumps(finish()),{}, {'completion_tokens':10000})
        row=s4.run_agent(self.case,'no_memory',TooMuch(),self.fixture,self.config)
        self.assertEqual(row['error'],'model_output_budget_exceeded')

    def test_fixture_summary_has_all_denominators(self):
        rows=[]
        for case in self.cases:
            for arm in s4.ARMS:
                tools=s4.GoldTools(self.corpus,self.labels) if arm=='gold_diagnostic' else self.fixture
                row=s4.run_agent(case,arm,s4.FixtureChat(),tools,self.config)
                row['metrics']=s4.assess(row,self.labels[case['id']]);rows.append(row)
        summary=s4.summarize(rows,self.cases,'fixture')
        self.assertEqual(summary['observed_rows'],40)
        for metrics in summary['arms'].values():
            self.assertEqual(metrics['cases'],10);self.assertEqual(metrics['no_answer_cases'],3)
            self.assertEqual(metrics['task_success_heuristic']['denominator'],10)
            self.assertEqual(metrics['required_source_coverage']['denominator'],10)
            self.assertIsNone(metrics['cost_usd'])
            self.assertEqual(metrics['semantic_entailment'],'NOT_EVALUATED')
        with self.assertRaisesRegex(s4.HarnessError,'incomplete_or_duplicate_rows'):s4.summarize(rows[:-1],self.cases,'fixture')
        with self.assertRaisesRegex(s4.HarnessError,'incomplete_or_duplicate_rows'):s4.summarize(rows+[rows[0]],self.cases,'fixture')

    def test_plan_not_run_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as tmp:
            output=Path(tmp)/'plan'
            self.assertEqual(s4.main(['--output',str(output)]),0)
            self.assertEqual(json.loads((output/'summary.json').read_text())['status'],'NOT_RUN')
            self.assertEqual(json.loads((output/'manifest.json').read_text())['live_model_status'],'NOT_RUN')
            with self.assertRaises(SystemExit),contextlib.redirect_stderr(io.StringIO()):s4.main(['--output',str(output)])

    def test_live_disabled_before_network(self):
        with tempfile.TemporaryDirectory() as tmp,patch.object(s4.LocalHTTP,'request',side_effect=AssertionError('network attempted')):
            output=Path(tmp)/'run'
            self.assertEqual(s4.main(['--mode','live','--output',str(output)]),2)
            self.assertEqual(json.loads((output/'summary.json').read_text())['status'],'NOT_RUN')


if __name__=='__main__':unittest.main()
