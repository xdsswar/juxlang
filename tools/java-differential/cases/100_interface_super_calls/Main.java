import java.util.function.Supplier;

interface Walker {
    default String move() { return "walking"; }
    default String describe(String who) { return who + " walks"; }
}

interface Swimmer {
    default String move() { return "swimming"; }
}

interface Picker<T> {
    default T pick(T first, T second) { return first; }
}

class Penguin implements Walker, Swimmer, Picker<Integer> {
    @Override
    public String move() {
        return Walker.super.move() + " and " + Swimmer.super.move();
    }

    @Override
    public String describe(String who) {
        return "Penguin: " + Walker.super.describe(who);
    }

    @Override
    public Integer pick(Integer first, Integer second) {
        return Picker.super.pick(second, first);
    }

    public String later() {
        Supplier<String> go = () -> Swimmer.super.move();
        return go.get();
    }
}

public class Main {
    public static void main(String[] args) {
        Penguin p = new Penguin();
        System.out.println(p.move());
        System.out.println(p.describe("Pingu"));
        System.out.println(p.pick(1, 2));
        System.out.println(p.later());
        Walker w = p;
        System.out.println(w.move());
    }
}
