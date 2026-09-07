public class Main {
    static class Animal {
        protected String name;
        Animal(String name) { this.name = name; }
        String speak() { return "..."; }
        String describe() { return this.name + " says " + this.speak(); }
    }
    static class Dog extends Animal {
        Dog(String n) { super(n); }
        String speak() { return "woof"; }
    }
    static class Puppy extends Dog {
        Puppy(String n) { super(n); }
        String speak() { return "yip (" + super.speak() + ")"; }
    }

    public static void main(String[] args) {
        Animal a = new Animal("thing");
        Animal d = new Dog("Rex");
        Animal p = new Puppy("Bit");
        System.out.println(a.describe());
        System.out.println(d.describe());
        System.out.println(p.describe());
        System.out.println(d.speak());
        System.out.println(p.speak());
    }
}
