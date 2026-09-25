import java.util.ArrayList;
import java.util.List;

public class Main {
    static class Animal {
        public String nm;
        Animal(String nm) { this.nm = nm; }
        String describe() { return "animal " + this.nm; }
        @Override
        public String toString() { return "Animal(" + this.nm + ")"; }
    }
    static class Dog extends Animal {
        Dog(String nm) { super(nm); }
        @Override
        String describe() { return "dog " + this.nm; }
    }
    static class Rock {
        public String label;
        public int weight;
        Rock(String label, int weight) { this.label = label; this.weight = weight; }
        String tag() { return "rock:" + this.label; }
    }

    static void reportAnimals(List<? extends Animal> xs) {
        for (Animal a : xs) {
            System.out.println(a.nm + " -> " + a.describe());
        }
    }

    static void reportRocks(List<? extends Rock> xs) {
        for (Rock r : xs) {
            System.out.println(r.label + " " + r.weight + " -> " + r.tag());
        }
    }

    static int totalNameLength(List<? extends Animal> xs) {
        int n = 0;
        for (Animal a : xs) {
            n += a.describe().length();
        }
        return n;
    }

    static void showFirstConsumer(List<? super Dog> v) {
        System.out.println("super " + v.get(0));
    }

    static void showFirstConcrete(List<Animal> v) {
        System.out.println("plain " + v.get(0));
    }

    public static void main(String[] args) {
        List<Dog> dogs = new ArrayList<>();
        dogs.add(new Dog("Rex"));
        dogs.add(new Dog("Ada"));

        List<Animal> animals = new ArrayList<>();
        animals.add(new Animal("Generic"));

        List<Rock> rocks = new ArrayList<>();
        rocks.add(new Rock("granite", 12));

        reportAnimals(dogs);
        reportAnimals(animals);
        reportRocks(rocks);
        System.out.println(totalNameLength(dogs));
        System.out.println(totalNameLength(animals));
        showFirstConsumer(animals);
        showFirstConcrete(animals);
    }
}
