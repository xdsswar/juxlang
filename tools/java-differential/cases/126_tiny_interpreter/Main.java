import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;

public class Main {
    sealed interface Op permits Push, Add, Mul, Sub, Dup, Swap, Over, Jz, Jmp, Label, Call, Ret, Print, Halt {}
    record Push(long value) implements Op {}
    record Add() implements Op {}
    record Mul() implements Op {}
    record Sub() implements Op {}
    record Dup() implements Op {}
    record Swap() implements Op {}
    record Over() implements Op {}
    record Jz(String target) implements Op {}
    record Jmp(String target) implements Op {}
    record Label(String name) implements Op {}
    record Call(String target) implements Op {}
    record Ret() implements Op {}
    record Print(String tag) implements Op {}
    record Halt() implements Op {}

    static class Machine {
        private final List<Op> code;
        private final ArrayList<Long> stack = new ArrayList<>();
        private final ArrayList<Integer> frames = new ArrayList<>();
        private final HashMap<String, Integer> labels = new HashMap<>();
        int steps = 0;

        Machine(List<Op> code) {
            this.code = code;
            for (int i = 0; i < code.size(); i++) {
                Op op = code.get(i);
                if (op instanceof Label l) {
                    labels.put(l.name(), i);
                }
            }
        }

        private long pop() {
            if (stack.isEmpty()) {
                throw new IllegalStateException("stack underflow at step " + steps);
            }
            return stack.remove(stack.size() - 1);
        }

        private int jump(String target) {
            Integer at = labels.get(target);
            if (at == null) {
                throw new IllegalArgumentException("no label " + target);
            }
            return at;
        }

        void run() {
            int pc = 0;
            while (pc < code.size()) {
                steps++;
                if (steps > 10000) {
                    throw new IllegalStateException("step limit");
                }
                Op op = code.get(pc);
                pc++;
                switch (op) {
                    case Push p -> stack.add(p.value());
                    case Add a -> { long y = pop(); long x = pop(); stack.add(x + y); }
                    case Mul m -> { long y = pop(); long x = pop(); stack.add(x * y); }
                    case Sub s -> { long y = pop(); long x = pop(); stack.add(x - y); }
                    case Dup d -> { long x = pop(); stack.add(x); stack.add(x); }
                    case Swap s -> { long y = pop(); long x = pop(); stack.add(y); stack.add(x); }
                    case Over o -> { long y = pop(); long x = pop(); stack.add(x); stack.add(y); stack.add(x); }
                    case Jz j -> { if (pop() == 0) { pc = jump(j.target()); } }
                    case Jmp j -> pc = jump(j.target());
                    case Label l -> {}
                    case Call c -> { frames.add(pc); pc = jump(c.target()); }
                    case Ret r -> {
                        if (frames.isEmpty()) {
                            throw new IllegalStateException("return with no caller");
                        }
                        pc = frames.remove(frames.size() - 1);
                    }
                    case Print p -> System.out.println(p.tag() + " " + pop());
                    case Halt h -> pc = code.size();
                }
            }
        }
    }

    static List<Op> factorial(long n) {
        List<Op> p = new ArrayList<>();
        p.add(new Push(1));
        p.add(new Push(n));
        p.add(new Label("loop"));
        p.add(new Dup());
        p.add(new Jz("done"));
        p.add(new Call("step"));
        p.add(new Push(1));
        p.add(new Sub());
        p.add(new Jmp("loop"));
        p.add(new Label("done"));
        p.add(new Swap());
        p.add(new Print("fact(" + n + ") ="));
        p.add(new Halt());
        p.add(new Label("step"));
        p.add(new Swap());
        p.add(new Over());
        p.add(new Mul());
        p.add(new Swap());
        p.add(new Ret());
        return p;
    }

    static List<Op> countdown(long from) {
        List<Op> p = new ArrayList<>();
        p.add(new Push(from));
        p.add(new Label("top"));
        p.add(new Dup());
        p.add(new Print("tick"));
        p.add(new Push(1));
        p.add(new Sub());
        p.add(new Dup());
        p.add(new Jz("end"));
        p.add(new Jmp("top"));
        p.add(new Label("end"));
        p.add(new Halt());
        return p;
    }

    static void attempt(String name, List<Op> program) {
        Machine m = new Machine(program);
        try {
            m.run();
            System.out.println(name + ": ok in " + m.steps + " steps");
        } catch (IllegalStateException e) {
            System.out.println(name + ": failed, " + e.getMessage());
        } catch (IllegalArgumentException e) {
            System.out.println(name + ": bad program, " + e.getMessage());
        }
    }

    public static void main(String[] args) {
        attempt("countdown", countdown(3));
        attempt("factorial", factorial(5));
        List<Op> broken = new ArrayList<>();
        broken.add(new Add());
        attempt("underflow", broken);
        List<Op> lost = new ArrayList<>();
        lost.add(new Jmp("nowhere"));
        attempt("jump", lost);
        List<Op> loop = new ArrayList<>();
        loop.add(new Label("x"));
        loop.add(new Jmp("x"));
        attempt("forever", loop);
    }
}
