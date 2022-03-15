// $(document).ready(function() {
//     $(".caret").click(() => {
//         console.log('parents', $(this).parents())
//         console.log('', $(this).parents().closest(".nested"))
//         $(this).parents().closest(".nested").toggleClass("active")
//         $(this).toggleClass("caret-down")
//     });
// })

document.addEventListener("DOMContentLoaded", function(){
    var toggler = document.getElementsByClassName("caret");
    var i;

    for (i = 0; i < toggler.length; i++) {
      toggler[i].addEventListener("click", function() {
        this.parentElement.querySelector(".nested").classList.toggle("active");
        this.classList.toggle("caret-down");
      });
    }
});
