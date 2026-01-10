# Has `num:/d/` to 3 paragraphs

`test:/test\d/`{1,3}

|c2|c2|
|-|-|
|a1|b1|
|a2|b2|

---

Output is

```json
{
"num": "1",
"test": ["test1", "test2", "test3"]
"a": ["a1", "a2"], "b": ["b1", "b2"]
}
```
