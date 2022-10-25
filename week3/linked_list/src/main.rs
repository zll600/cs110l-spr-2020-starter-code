use linked_list::LinkedList;
pub mod linked_list;

fn main() {
    let mut list: LinkedList<u32> = LinkedList::new();
    assert!(list.is_empty());
    assert_eq!(list.get_size(), 0);
    for i in 1..12 {
        list.push_front(i);
    }

    println!("{}", list);
    println!("list size: {}", list.get_size());
    println!("top element: {}", list.pop_front().unwrap());
    println!("{}", list);
    println!("size: {}", list.get_size());
    println!("{}", list.to_string()); // ToString impl for anything impl Display

    let mut str_list: LinkedList<String> = LinkedList::new();
    str_list.push_front(String::from("hello"));
    str_list.push_front(String::from("world"));
    println!("{}", str_list);
    let str_list_clone = str_list.clone();
    println!("{}", str_list_clone);
    println!("is same: {}", str_list == str_list_clone);

    // If you implement iterator trait:
    //for val in &list {
    //    println!("{}", val);
    //}
}
