const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

// Exercise the actual boolean/output expressions rather than a copied policy.
const action = fs.readFileSync(path.join(__dirname, '../actions/ci-changes/action.yml'), 'utf8');
const workflow = fs.readFileSync(path.join(__dirname, '../workflows/titan.yml'), 'utf8');
const evaluate = (expression, context) => vm.runInNewContext(expression.replaceAll('steps.titan-filter', 'steps.titanFilter'), {
  always: () => true, cancelled: () => false, ...context,
});
const actionOutput = (name, source = action) => source.replace(/\r\n/g, '\n')
  .match(new RegExp(`  ${name}:\\n[\\s\\S]*?value: \\$\\{\\{ (.*?) \\}\\}`))[1];
const packageExpression = workflow.match(/TITAN_PACKAGES: \$\{\{ (.*?) \}\}/)[1];

test('upstream and ECS escalation includes non-Titan consumers and missing classifier output', () => {
  for (const flag of ['true', undefined, '']) {
    const context = { steps: { filter: { outputs: { upstream: 'false', ecs: 'false' } }, titanFilter: { outputs: { upstream: flag } } } };
    for (const source of [action, action.replace(/\r?\n/g, '\r\n')]) {
      assert.equal(evaluate(actionOutput('upstream', source), context), true);
      assert.equal(evaluate(actionOutput('ecs', source), context), true);
    }
  }
  const context = { steps: { filter: { outputs: { upstream: 'false', ecs: 'false' } }, titanFilter: { outputs: { upstream: 'false' } } } };
  assert.equal(evaluate(actionOutput('upstream'), context), false);
  assert.equal(evaluate(actionOutput('ecs'), context), false);
});

test('failed/missing selection runs full; none still emits the required check and avoids setup/tests', () => {
  const guard = workflow.match(/  titan-tests:[\s\S]*?    if: \$\{\{ (.*?) \}\}/)[1];
  for (const result of ['success', 'failure', 'skipped']) {
    const context = { needs: { changes: { result, outputs: {} } } };
    assert.equal(evaluate(guard, context), true);
    assert.equal(evaluate(packageExpression, context), '*');
  }
  const context = { needs: { changes: { result: 'success', outputs: { titan_packages: 'none', upstream: 'false' } } }, env: { TITAN_PACKAGES: 'none' } };
  assert.equal(evaluate(packageExpression, context), 'none');
  assert.equal(evaluate(guard, context), true);
  for (const expression of workflow.matchAll(/        if: \$\{\{ (.*?) \}\}/g)) {
    assert.equal(evaluate(expression[1], context), expression[1].includes("== 'none'"), expression[1]);
  }
  context.needs.changes.outputs.titan_packages = 'titan_determinism';
  assert.equal(evaluate(packageExpression, context), 'titan_determinism');
});
