public class Main {
    static class Slot<T> {
        private T value;
        boolean isEmpty() { return this.value == null; }
        void put(T v) { this.value = v; }
        T peek() { return this.value; }
        String describe() {
            if (this.value == null) { return "empty"; }
            return "has " + this.value;
        }
    }
    static class Item {
        String name;
        Item(String n) { this.name = n; }
    }

    public static void main(String[] args) {
        Slot<String> s = new Slot<>();
        System.out.println(s.isEmpty());
        System.out.println(s.describe());
        s.put("v");
        System.out.println(s.isEmpty());
        System.out.println(s.describe());
        System.out.println(s.peek() == null ? "none" : s.peek());

        Slot<Item> o = new Slot<>();
        System.out.println(o.describe());
        o.put(new Item("thing"));
        System.out.println(o.isEmpty());
        System.out.println(o.peek().name);

        Item held = o.peek();
        held.name = "changed";
        System.out.println(o.peek().name);

        Slot<Slot<String>> ss = new Slot<>();
        System.out.println(ss.isEmpty());
        ss.put(new Slot<>());
        System.out.println(ss.isEmpty());
        System.out.println(ss.peek().isEmpty());
        ss.peek().put("deep");
        System.out.println(ss.peek().describe());
    }
}
