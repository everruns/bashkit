if False:
    pass
elif True:
    raise ValueError('in elif body')
"""
TRACEBACK:
Traceback (most recent call last):
  File "if__raise_elif.py", line 4, in <module>
    raise ValueError('in elif body')
ValueError: in elif body
"""
