#!/usr/bin/env python3
import json, os, sys
jobs = json.load(sys.stdin)
for job in jobs:
    with open(os.environ['YAP_TEST_PUSH_ROUTE'], 'w') as out:
        json.dump({'route': job['route'], 'revision': str(job['seq'])}, out)
print(json.dumps({job['route']: 'ok' for job in jobs}))
