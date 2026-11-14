"""lcnet100 with each BiFPN `Einsum('i,i...->...', w, x)` (a weighted sum
over a stack) written as Mul and ReduceSum, which every runtime has."""
import numpy as np, onnx, onnxruntime as ort, sys
from onnx import helper, numpy_helper
src, dst = sys.argv[1], sys.argv[2]
m = onnx.load(src)
opset = next(o.version for o in m.opset_import if o.domain in ("", "ai.onnx"))
print("opset", opset)
g = m.graph
axes = numpy_helper.from_array(np.array([1, 2, 3, 4], np.int64), "einsum_w_axes")
zero = numpy_helper.from_array(np.array([0], np.int64), "einsum_sum_axis")
g.initializer.extend([axes, zero])
nodes = []
for n in g.node:
    if n.op_type != "Einsum":
        nodes.append(n); continue
    w, x = n.input; y = n.output[0]
    nodes.append(helper.make_node("Unsqueeze", [w, "einsum_w_axes"], [y + "_w"], name=n.name + "_w"))
    nodes.append(helper.make_node("Mul", [y + "_w", x], [y + "_m"], name=n.name + "_m"))
    nodes.append(helper.make_node("ReduceSum", [y + "_m", "einsum_sum_axis"], [y], name=n.name + "_s", keepdims=0))
del g.node[:]; g.node.extend(nodes)
onnx.checker.check_model(m)
onnx.save(m, dst)
x = np.random.rand(1, 3, 256, 256).astype(np.float32)
a = ort.InferenceSession(src).run(None, {"img": x})[0]
b = ort.InferenceSession(dst).run(None, {"img": x})[0]
print("max difference", float(np.abs(a - b).max()))
