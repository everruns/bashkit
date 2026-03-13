def fail():
    raise ValueError('elif condition failed')


if False:
    x = 1
elif fail():
    x = 2
"""
TRACEBACK:
Traceback (most recent call last):
  File "if__raise_in_elif_condition.py", line 7, in <module>
    elif fail():
         ~~~~~~
  File "if__raise_in_elif_condition.py", line 2, in fail
    raise ValueError('elif condition failed')
ValueError: elif condition failed
"""
