import assert from 'node:assert/strict';
import test from 'node:test';
import { compileModules } from './typescript.mjs';

const modules = await compileModules(['src/numberExpression.ts']);
try {
  const { evaluateNumberExpression, clampNumber, roundNumber, resolveNumberField } = await modules.load('numberExpression.js');

  await test('arithmetic expressions follow precedence and parentheses', () => {
    assert.equal(evaluateNumberExpression('120*2'), 240);
    assert.equal(evaluateNumberExpression('2+3*4'), 14);
    assert.equal(evaluateNumberExpression('(10+5)*2'), 30);
    assert.equal(evaluateNumberExpression('100/3'), 33.33);
    assert.equal(evaluateNumberExpression('10 - 4 / 2'), 8);
    assert.equal(evaluateNumberExpression('2*(3+(4-1))'), 12);
  });

  await test('decimals and unary minus parse', () => {
    assert.equal(evaluateNumberExpression('.5*2'), 1);
    assert.equal(evaluateNumberExpression('12.'), 12);
    assert.equal(evaluateNumberExpression('-8'), -8);
    assert.equal(evaluateNumberExpression('- 8'), -8);
    assert.equal(evaluateNumberExpression('--8'), 8);
    assert.equal(evaluateNumberExpression('2*-3'), -6);
    assert.equal(evaluateNumberExpression('-(2+3)'), -5);
    assert.equal(evaluateNumberExpression('+16'), 16);
  });

  await test('leading +, * and / are relative to the current value', () => {
    assert.equal(evaluateNumberExpression('+16', 100), 116);
    assert.equal(evaluateNumberExpression('*2', 100), 200);
    assert.equal(evaluateNumberExpression('/2', 100), 50);
    assert.equal(evaluateNumberExpression('+-8', 100), 92);
    assert.equal(evaluateNumberExpression('+(-8)', 100), 92);
    assert.equal(evaluateNumberExpression('*(1+1)', 30), 60);
    assert.equal(evaluateNumberExpression('/4', 10), 2.5);
    assert.equal(evaluateNumberExpression('+16', null), 16);
    assert.equal(evaluateNumberExpression('*2', null), null);
    assert.equal(evaluateNumberExpression('/2', null), null);
    assert.equal(evaluateNumberExpression('/0', 100), null);
  });

  await test('a leading minus is an absolute number, even in a field with a value', () => {
    assert.equal(evaluateNumberExpression('-8', 100), -8);
    assert.equal(evaluateNumberExpression('-8.5', 100), -8.5);
  });

  await test('units typed by habit are ignored, including percentages', () => {
    assert.equal(evaluateNumberExpression('24px'), 24);
    assert.equal(evaluateNumberExpression('50%', 100), 50);
    assert.equal(evaluateNumberExpression('12.5rem'), 12.5);
    assert.equal(evaluateNumberExpression('90deg'), 90);
    assert.equal(evaluateNumberExpression('24px + 8px'), 32);
    assert.equal(evaluateNumberExpression('50% + 10'), 60);
  });

  await test('invalid input returns null instead of a number', () => {
    for (const draft of ['', '   ', 'abc', '1+', '(2+3', '2**3', '1/0', '5 5', '()', '50%2', 'Infinity', 'NaN', '0x10', '1,5']) {
      assert.equal(evaluateNumberExpression(draft), null, draft);
    }
  });

  await test('results round to two decimals and normalise negative zero', () => {
    assert.equal(evaluateNumberExpression('2/3'), 0.67);
    assert.equal(evaluateNumberExpression('-2/3'), -0.67);
    assert.equal(roundNumber(33.333333), 33.33);
    assert.equal(String(roundNumber(-0.001)), '0');
  });

  await test('clamping keeps values inside the field range', () => {
    assert.equal(clampNumber(120, 0, 100), 100);
    assert.equal(clampNumber(-4, 0, 100), 0);
    assert.equal(clampNumber(40, 0, 100), 40);
    assert.equal(clampNumber(40, undefined, 30), 30);
  });

  await test('field drafts resolve to commit strings, or null when invalid', () => {
    assert.equal(resolveNumberField('120*2', 10), '240');
    assert.equal(resolveNumberField('(10+5)*2', 0), '30');
    assert.equal(resolveNumberField('100/3', 0), '33.33');
    assert.equal(resolveNumberField('24px', 0), '24');
    assert.equal(resolveNumberField('50%', 100, 0, 100), '50');
    assert.equal(resolveNumberField('+16', 100), '116');
    assert.equal(resolveNumberField('+-8', 100), '92');
    assert.equal(resolveNumberField('-8', 100), '-8');
    assert.equal(resolveNumberField('200', 50, 0, 100), '100');
    assert.equal(resolveNumberField('-20', 50, 0, 100), '0');
    assert.equal(resolveNumberField('+16', ''), '16');
    assert.equal(resolveNumberField('', 50), '');
    assert.equal(resolveNumberField('abc', 50), null);
    assert.equal(resolveNumberField('1/0', 50), null);
    assert.equal(resolveNumberField('*2', ''), null);
  });
} finally {
  await modules.cleanup();
}
