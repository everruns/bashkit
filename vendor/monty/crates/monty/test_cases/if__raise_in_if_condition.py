def fail():
    raise ValueError('condition failed')


if fail():
    x = 1
"""
TRACEBACK:
Traceback (most recent call last):
  File "if__raise_in_if_condition.py", line 5, in <module>
    if fail():
       ~~~~~~
  File "if__raise_in_if_condition.py", line 2, in fail
    raise ValueError('condition failed')
ValueError: condition failed
"""
